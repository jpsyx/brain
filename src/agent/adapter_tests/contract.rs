use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::{
    agent::{
        AgentController, AgentError, AgentKind, AgentObservationBoundary, AgentObservationCursor,
        AgentObservationPhase, AgentObservationRequest, AgentObservationResult, AgentTransport,
        LaunchSpec, OpenCodeFrontend, SessionScope, SessionStore,
    },
    state::Db,
    workspace::{CommandContext, MachineRegistry, RegistryStore, WorkspaceRecord},
};

struct ObservationTransport;

impl AgentTransport for ObservationTransport {
    fn spawn(&mut self, _spec: &LaunchSpec) -> Result<(), AgentError> {
        Ok(())
    }

    fn send(&mut self, _input: InputSequence) -> Result<(), AgentError> {
        Ok(())
    }

    fn snapshot(&self) -> String {
        String::new()
    }

    fn is_alive(&self) -> bool {
        true
    }

    fn shutdown(&mut self) -> Result<(), AgentError> {
        Ok(())
    }
}

struct FrontendContract {
    kind: AgentKind,
    label: &'static str,
    configured_key: &'static str,
    configured_value: &'static str,
    frontend: fn(&str) -> Box<dyn AgentFrontend>,
    submit: &'static [u8],
    busy_turn_follow_up: &'static [u8],
    new_session: &'static [u8],
    fresh_command: &'static str,
    resume_command: &'static str,
    fresh_prompt_command: &'static str,
    resume_prompt_command: &'static str,
    completion: CompletionStrategy,
    receiver_resume: Option<bool>,
}

fn claude_frontend(command: &str) -> Box<dyn AgentFrontend> {
    Box::new(ClaudeFrontend::new(
        command,
        PathBuf::from("/workspaces/family brain"),
        PathBuf::from("/home/tester/.claude/projects"),
    ))
}

fn codex_frontend(command: &str) -> Box<dyn AgentFrontend> {
    Box::new(CodexFrontend::new(command))
}

fn opencode_frontend(command: &str) -> Box<dyn AgentFrontend> {
    Box::new(OpenCodeFrontend::new(command))
}

fn frontend_contracts() -> [FrontendContract; 3] {
    [
        FrontendContract {
            kind: AgentKind::Claude,
            label: "Claude",
            configured_key: "claude_cmd",
            configured_value: "claude-contract",
            frontend: claude_frontend,
            submit: b"\r",
            busy_turn_follow_up: b"\x1b[200~follow\x1b[201~\r",
            new_session: b"/new\r",
            fresh_command: "claude-contract --session-id 'fresh-1'",
            resume_command: "claude-contract --resume 'resume-1'",
            fresh_prompt_command: "claude-contract --session-id 'fresh-1' -- 'receiver prompt'",
            resume_prompt_command: "claude-contract --resume 'resume-1' -- 'receiver prompt'",
            completion: CompletionStrategy::Hook,
            receiver_resume: Some(true),
        },
        FrontendContract {
            kind: AgentKind::Codex,
            label: "Codex",
            configured_key: "codex_cmd",
            configured_value: "codex-contract",
            frontend: codex_frontend,
            submit: b"\r",
            busy_turn_follow_up: b"\x1b[200~follow\x1b[201~\t",
            new_session: b"/new\t",
            fresh_command: "codex-contract --dangerously-bypass-hook-trust",
            resume_command: "codex-contract --dangerously-bypass-hook-trust resume 'resume-1'",
            fresh_prompt_command: "codex-contract --dangerously-bypass-hook-trust -- 'receiver prompt'",
            resume_prompt_command: "codex-contract --dangerously-bypass-hook-trust resume 'resume-1' -- 'receiver prompt'",
            completion: CompletionStrategy::Hook,
            receiver_resume: Some(false),
        },
        FrontendContract {
            kind: AgentKind::OpenCode,
            label: "OpenCode",
            configured_key: "opencode_cmd",
            configured_value: "opencode-contract",
            frontend: opencode_frontend,
            submit: b"\r",
            busy_turn_follow_up: b"\x1b[200~follow\x1b[201~\r",
            new_session: b"/new\r",
            fresh_command: "opencode-contract --agent brain",
            resume_command: "opencode-contract --agent brain --session 'resume-1'",
            fresh_prompt_command: "opencode-contract --agent brain --prompt 'receiver prompt'",
            resume_prompt_command: "opencode-contract --agent brain --session 'resume-1' --prompt 'receiver prompt'",
            completion: CompletionStrategy::Hook,
            receiver_resume: None,
        },
    ]
}

#[test]
fn receiver_launch_adapter_contract_translates_both_plans_with_an_initial_prompt() {
    for case in frontend_contracts() {
        let frontend = (case.frontend)(case.configured_value);
        let fresh = request(
            SessionPlan::fresh(AgentSession::new("fresh-1").expect("fresh session")),
            Some("receiver prompt"),
        );
        let resume = request(
            SessionPlan::resume(AgentSession::new("resume-1").expect("resume session")),
            Some("receiver prompt"),
        );

        assert_eq!(
            frontend.launch_spec(&fresh).expect("fresh launch").command,
            case.fresh_prompt_command,
            "{} fresh receiver launch",
            case.label,
        );
        assert_eq!(
            frontend
                .launch_spec(&resume)
                .expect("resume launch")
                .command,
            case.resume_prompt_command,
            "{} resumed receiver launch",
            case.label,
        );
    }
}

#[test]
fn receiver_launch_commands_bound_quote_heavy_prompts_for_every_frontend_and_plan() {
    let prefix = "## Current authenticated message\nKeep Unicode é🙂 and apostrophes.\n\nAttachment references:\n- filename=\"quote-heavy-é🙂.txt\"\n";
    let prompt = format!(
        "{prefix}{}",
        "'".repeat(SHELL_INLINE_VALUE_BUDGET_BYTES - prefix.len())
    );

    assert_eq!(prompt.len(), SHELL_INLINE_VALUE_BUDGET_BYTES);
    let quoted_prompt = shell_quote(&prompt);
    assert!(
        quoted_prompt.len() <= SHELL_COMMAND_ARGUMENT_BUDGET_BYTES,
        "shell quoting alone must fit the transport-safe command argument"
    );

    for case in frontend_contracts() {
        let frontend = (case.frontend)(case.configured_value);
        for plan in [
            SessionPlan::fresh(AgentSession::new("fresh-1").expect("fresh session")),
            SessionPlan::resume(AgentSession::new("resume-1").expect("resume session")),
        ] {
            let spec = frontend
                .launch_spec(&workspace_only_request(plan, Some(&prompt)))
                .expect("receiver launch spec");

            assert!(
                spec.command.ends_with(&quoted_prompt),
                "{} prompt",
                case.label
            );
            let fixed_overhead = spec.command.len() - quoted_prompt.len();
            assert!(
                fixed_overhead <= SHELL_COMMAND_FIXED_OVERHEAD_BUDGET_BYTES,
                "{} fixed command and policy overhead was {fixed_overhead} bytes",
                case.label,
            );
            assert!(
                spec.command.len() <= SHELL_COMMAND_ARGUMENT_BUDGET_BYTES,
                "{} command was {} bytes",
                case.label,
                spec.command.len(),
            );
        }
    }
}

fn configured_command_context() -> (tempfile::TempDir, CommandContext) {
    let temporary = tempfile::tempdir().expect("temporary command context");
    let root = temporary.path().join("family");
    std::fs::create_dir(&root).expect("workspace root");
    let id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("workspace id");
    let name = WorkspaceName::parse("family").expect("workspace name");
    let env = serde_json::Map::from_iter([
        (
            "claude_cmd".to_owned(),
            serde_json::json!("claude-contract"),
        ),
        ("codex_cmd".to_owned(), serde_json::json!("codex-contract")),
        (
            "opencode_cmd".to_owned(),
            serde_json::json!("opencode-contract"),
        ),
    ]);
    let registry = MachineRegistry {
        schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
        default_workspace: name.clone(),
        workspaces: BTreeMap::from([(
            name.clone(),
            WorkspaceRecord {
                workspace_id: id,
                root: root.clone(),
                aliases: BTreeSet::new(),
                local_user_id: "pablo".to_owned(),
                receiver_enabled: false,
                env,
            },
        )]),
        env: serde_json::Map::new(),
    };
    let store = RegistryStore::from_path(temporary.path().join("env.json"));
    store.replace(&registry).expect("registry");
    let workspace =
        WorkspaceContext::new(temporary.path(), id, name, &root, "pablo", temporary.path())
            .expect("workspace context");
    let context = CommandContext::for_test(Arc::new(workspace), store, "pablo");
    (temporary, context)
}

#[test]
fn every_frontend_satisfies_the_current_characterization_contract() {
    let cases = frontend_contracts();
    assert_eq!(
        cases.iter().map(|case| case.kind).collect::<Vec<_>>(),
        AgentKind::ALL
    );
    let (_temporary, command_context) = configured_command_context();

    for case in cases {
        let frontend = (case.frontend)(case.configured_value);
        assert_eq!(case.kind.label(), case.label, "{} label", case.label);
        assert_eq!(
            crate::env::resolve_one(&command_context, case.configured_key).as_deref(),
            Some(case.configured_value),
            "{} configured key",
            case.label
        );
        assert_eq!(
            crate::agent::configured_command(&command_context, case.kind),
            case.configured_value,
            "{} configured command routing",
            case.label
        );
        assert_eq!(
            frontend.input_for(AgentAction::SubmitNow),
            Ok(InputSequence::bytes(case.submit)),
            "{} submit",
            case.label
        );
        let follow_up = frontend
            .input_for(AgentAction::FollowUpAfterActiveTurn("follow"))
            .expect("busy-turn follow-up");
        assert_eq!(
            follow_up.flattened(),
            case.busy_turn_follow_up.to_vec(),
            "{} busy-turn follow-up",
            case.label
        );
        // Only Claude was measured losing the submit when the key shares the
        // paste's write; Codex and OpenCode submitted either way. Every
        // frontend is paced anyway — the pacing costs one injected follow-up
        // 400ms, and which frontends have the flaw is a fact about their
        // current builds, not something to re-derive after each upgrade.
        assert_eq!(
            follow_up.writes().len(),
            2,
            "{} must submit a follow-up in its own write",
            case.label
        );
        assert!(
            follow_up.writes()[1].settle > std::time::Duration::ZERO,
            "{} must let the paste land before the submit key",
            case.label
        );
        assert_eq!(
            frontend.input_for(AgentAction::StartNewSession),
            Ok(InputSequence::bytes(case.new_session)),
            "{} new session",
            case.label
        );
        assert_eq!(
            frontend
                .launch_spec(&fresh("fresh-1"))
                .expect("fresh launch")
                .command,
            case.fresh_command,
            "{} fresh support",
            case.label
        );
        assert_eq!(
            frontend
                .launch_spec(&resume("resume-1"))
                .expect("resume launch")
                .command,
            case.resume_command,
            "{} resume support",
            case.label
        );
        assert_eq!(
            frontend.completion_strategy(),
            Ok(case.completion),
            "{} completion strategy",
            case.label
        );
        if let Some(receiver_resume) = case.receiver_resume {
            assert_eq!(
                frontend.can_resume_response_session(&AgentSession::new("resume-1").unwrap()),
                Ok(receiver_resume),
                "{} receiver resume",
                case.label
            );
        }
    }
}

#[test]
fn every_frontend_observes_the_same_normalized_lifecycle_contract() {
    let token = "6c06c55a-a9cf-4d75-b14e-75a5900c9088";
    let instance = "5cbd43f1-cc3f-4bc4-81ad-acad2bf85d39";
    let session = AgentSession::new("native-session-7").expect("native session");
    let expected = AgentObservationResult::new(
        session.clone(),
        vec![
            AgentObservationBoundary::new(AgentObservationPhase::Accepted, 1_000),
            AgentObservationBoundary::new(AgentObservationPhase::Progressing, 1_100),
            AgentObservationBoundary::new(AgentObservationPhase::Completed, 1_200),
        ],
        Some(crate::agent::AgentProgressPulse::new(1_100)),
        AgentObservationCursor::at_revision(3, Some(1_000), Some(1_100), Some(1_100), Some(1_200)),
    );

    for case in frontend_contracts() {
        let (_temporary, command) = configured_command_context();
        let db = Db::open(&command.workspace).expect("state database");
        let scope = SessionScope::new(case.kind, command.workspace.id(), actor());
        let prior = AgentSession::new("prior-native-session").expect("prior session");
        SessionStore::register(&db, &prior, instance, 41, &scope).expect("prior ownership");
        SessionStore::release(&db, instance).expect("rotate prior session");
        SessionStore::register(&db, &session, instance, 42, &scope).expect("owned native session");
        let path = command
            .workspace
            .paths()
            .receiver_observations_dir()
            .join(format!("{instance}.json"));
        std::fs::create_dir_all(path.parent().expect("observation parent"))
            .expect("observation directory");
        std::fs::write(
            &path,
            format!(
                r#"{{"version":1,"revision":3,"phase":"completed","job_token":"{token}","instance_id":"{instance}","session_id":"{}","turn_id":"turn-9","accepted_at_unix_ms":1000,"progressing_at_unix_ms":1100,"latest_progress_at_unix_ms":1100,"completed_at_unix_ms":1200}}"#,
                session.as_str()
            ),
        )
        .expect("observation snapshot");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("owner-only observation");
        }
        let controller = AgentController::new(
            Arc::clone(&command.workspace),
            actor(),
            (case.frontend)(case.configured_value),
            Box::new(ObservationTransport),
        );
        let request = AgentObservationRequest::new(
            token,
            instance,
            path.clone(),
            session.clone(),
            AgentObservationCursor::launched(),
        );

        assert_eq!(
            controller.observe(&request),
            Ok(expected.clone()),
            "{} normalized observation",
            case.label
        );
        let prior_request = AgentObservationRequest::new(
            token,
            instance,
            path.clone(),
            prior,
            AgentObservationCursor::launched(),
        );
        assert_eq!(
            controller.observe(&prior_request),
            Err(crate::agent::AgentObservationError::SessionOwnership),
            "{} prior rotated session",
            case.label
        );
        let placeholder_request = AgentObservationRequest::new(
            token,
            instance,
            path,
            AgentSession::new("pending-receiver-5cbd43f1-cc3f-4bc4-81ad-acad2bf85d39")
                .expect("placeholder"),
            AgentObservationCursor::launched(),
        );
        assert_eq!(
            controller.observe(&placeholder_request),
            Err(crate::agent::AgentObservationError::PlaceholderSession),
            "{} placeholder session",
            case.label
        );
    }
}

#[test]
fn every_frontend_observes_a_newer_progress_pulse_without_a_new_phase() {
    let token = "6c06c55a-a9cf-4d75-b14e-75a5900c9088";
    let instance = "5cbd43f1-cc3f-4bc4-81ad-acad2bf85d39";
    let session = AgentSession::new("native-session-7").expect("native session");

    for case in frontend_contracts() {
        let (_temporary, command) = configured_command_context();
        let db = Db::open(&command.workspace).expect("state database");
        let scope = SessionScope::new(case.kind, command.workspace.id(), actor());
        SessionStore::register(&db, &session, instance, 42, &scope).expect("owned session");
        let path = command
            .workspace
            .paths()
            .receiver_observations_dir()
            .join(format!("{instance}.json"));
        std::fs::create_dir_all(path.parent().expect("observation parent"))
            .expect("observation directory");
        std::fs::write(
            &path,
            format!(
                r#"{{"version":1,"revision":3,"phase":"progressing","job_token":"{token}","instance_id":"{instance}","session_id":"{}","turn_id":"turn-10","accepted_at_unix_ms":1000,"progressing_at_unix_ms":1100,"latest_progress_at_unix_ms":1200,"completed_at_unix_ms":null}}"#,
                session.as_str()
            ),
        )
        .expect("observation snapshot");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("owner-only observation");
        }
        let controller = AgentController::new(
            Arc::clone(&command.workspace),
            actor(),
            (case.frontend)(case.configured_value),
            Box::new(ObservationTransport),
        );
        let cursor =
            AgentObservationCursor::from_durable(2, Some(1_000), Some(1_100), Some(1_100), None)
                .expect("durable progress cursor");
        let result = controller
            .observe(&AgentObservationRequest::new(
                token,
                instance,
                path,
                session.clone(),
                cursor,
            ))
            .expect("newer progress pulse");

        assert!(result.boundaries().is_empty(), "{} phase", case.label);
        assert_eq!(
            result
                .progress_pulse()
                .expect("frontend progress pulse")
                .observed_at_unix_ms(),
            1_200,
            "{} pulse",
            case.label
        );
    }
}

#[test]
fn controller_rejects_invalid_identity_and_paths_before_ownership_lookup() {
    let token = "6c06c55a-a9cf-4d75-b14e-75a5900c9088";
    let instance = "5cbd43f1-cc3f-4bc4-81ad-acad2bf85d39";
    let (_temporary, command) = configured_command_context();
    Db::open(&command.workspace).expect("state database");
    let controller = AgentController::new(
        Arc::clone(&command.workspace),
        actor(),
        claude_frontend("claude-contract"),
        Box::new(ObservationTransport),
    );
    let path = command
        .workspace
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"));
    for (label, request, expected) in [
        (
            "overlong session",
            AgentObservationRequest::new(
                token,
                instance,
                path.clone(),
                AgentSession::new("s".repeat(257)).expect("nonblank session"),
                AgentObservationCursor::launched(),
            ),
            crate::agent::AgentObservationError::InvalidIdentifier,
        ),
        (
            "noncanonical token",
            AgentObservationRequest::new(
                "6C06C55A-A9CF-4D75-B14E-75A5900C9088",
                instance,
                path.clone(),
                AgentSession::new("native-session").expect("session"),
                AgentObservationCursor::launched(),
            ),
            crate::agent::AgentObservationError::InvalidIdentifier,
        ),
        (
            "noncanonical instance",
            AgentObservationRequest::new(
                token,
                "not-an-instance",
                path.clone(),
                AgentSession::new("native-session").expect("session"),
                AgentObservationCursor::launched(),
            ),
            crate::agent::AgentObservationError::InvalidIdentifier,
        ),
        (
            "wrong canonical path",
            AgentObservationRequest::new(
                token,
                instance,
                path.with_file_name("wrong.json"),
                AgentSession::new("native-session").expect("session"),
                AgentObservationCursor::launched(),
            ),
            crate::agent::AgentObservationError::WrongPath,
        ),
    ] {
        assert_eq!(controller.observe(&request), Err(expected), "{label}");
    }
}

#[test]
fn controller_discards_observations_when_ownership_rotates_during_the_poll() {
    let token = "6c06c55a-a9cf-4d75-b14e-75a5900c9088";
    let instance = "5cbd43f1-cc3f-4bc4-81ad-acad2bf85d39";
    let replacement_instance = "34a78cb6-abf8-456f-b406-39abac6d569a";
    let session = AgentSession::new("native-session-7").expect("native session");
    let replacement = AgentSession::new("native-session-8").expect("replacement session");
    let (_temporary, command) = configured_command_context();
    let db = Db::open(&command.workspace).expect("state database");
    let ownership_scope = SessionScope::new(AgentKind::Claude, command.workspace.id(), actor());
    SessionStore::register(&db, &session, instance, 41, &ownership_scope)
        .expect("initial ownership");
    let path = command
        .workspace
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"));
    std::fs::create_dir_all(path.parent().expect("observation parent"))
        .expect("observation directory");
    std::fs::write(
        &path,
        format!(
            r#"{{"version":1,"revision":1,"phase":"accepted","job_token":"{token}","instance_id":"{instance}","session_id":"{}","turn_id":null,"accepted_at_unix_ms":1000,"progressing_at_unix_ms":null,"latest_progress_at_unix_ms":null,"completed_at_unix_ms":null}}"#,
            session.as_str()
        ),
    )
    .expect("observation snapshot");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only observation");
    }
    let controller = AgentController::new(
        Arc::clone(&command.workspace),
        actor(),
        claude_frontend("claude-contract"),
        Box::new(ObservationTransport),
    );
    let request = AgentObservationRequest::new(
        token,
        instance,
        path,
        session,
        AgentObservationCursor::launched(),
    );

    assert_eq!(
        controller.observe_with_post_read_hook(&request, || {
            let workspace = Arc::clone(&command.workspace);
            let replacement = replacement.clone();
            std::thread::spawn(move || {
                let rotated_db = Db::open(&workspace).expect("fresh durable state");
                let rotated_scope = SessionScope::new(AgentKind::Claude, workspace.id(), actor());
                SessionStore::release(&rotated_db, instance).expect("release observed instance");
                SessionStore::register(
                    &rotated_db,
                    &replacement,
                    replacement_instance,
                    42,
                    &rotated_scope,
                )
                .expect("rotated ownership");
            })
            .join()
            .expect("rotation thread");
        }),
        Err(crate::agent::AgentObservationError::SessionOwnership)
    );
}
