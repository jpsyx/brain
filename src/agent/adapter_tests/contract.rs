use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::{
    agent::{AgentKind, OpenCodeFrontend},
    workspace::{CommandContext, MachineRegistry, RegistryStore, WorkspaceRecord},
};

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
