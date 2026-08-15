use super::*;
use std::path::PathBuf;

fn workspace() -> crate::workspace::WorkspaceContext {
    crate::workspace::WorkspaceContext::new(
        std::path::Path::new("/home/tester"),
        crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
            .expect("valid id"),
        crate::workspace::WorkspaceName::parse("family").expect("valid name"),
        std::path::Path::new("/home/tester/family"),
        "pablo",
        std::path::Path::new("/home/tester"),
    )
    .expect("context")
}

fn actor() -> crate::actor::ActorContext {
    let users = crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: crate::users::UserId::parse("pablo").unwrap(),
            name: "Pablo".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    crate::actor::resolve_actor(
        &crate::users::UserId::parse("pablo").unwrap(),
        crate::actor::RequestIdentity::Local,
        &users,
    )
    .unwrap()
}

#[test]
fn decide_resumes_when_a_candidate_exists() {
    let plan = Plan::decide(Some("abc-123".to_owned()), "new-id".to_owned());
    assert_eq!(plan, Plan::Resume("abc-123".to_owned()));
}

#[test]
fn decide_starts_fresh_when_nothing_to_resume() {
    let plan = Plan::decide(None, "new-id".to_owned());
    assert_eq!(plan, Plan::Fresh("new-id".to_owned()));
}

#[test]
fn fresh_command_uses_session_id_flag() {
    let cmd = build_llm_command(
        &PathBuf::from("/Users/x/brain"),
        AgentKind::Claude,
        "claude",
        &Plan::Fresh("uuid-1".to_owned()),
        None,
    )
    .expect("Claude fresh command");
    assert!(cmd.starts_with("cd '/Users/x/brain' && claude"));
    assert!(cmd.contains("--session-id 'uuid-1'"));
    assert!(!cmd.contains("--resume"));
}

#[test]
fn resume_command_uses_resume_flag() {
    let cmd = build_llm_command(
        &PathBuf::from("/Users/x/brain"),
        AgentKind::Claude,
        "claude",
        &Plan::Resume("sess-9".to_owned()),
        None,
    )
    .expect("Claude resume command");
    assert!(cmd.contains("--resume 'sess-9'"));
    assert!(!cmd.contains("--session-id"));
}

#[test]
fn configured_command_is_spliced_in_before_brains_own_flags() {
    // The configured command may carry its own flags; brain's --resume must
    // come after them, and the command is not shell-quoted (the shell
    // interprets its flags).
    let cmd = build_llm_command(
        &PathBuf::from("/Users/x/brain"),
        AgentKind::Claude,
        "claude --dangerously-skip-permissions",
        &Plan::Resume("sess-9".to_owned()),
        None,
    )
    .expect("configured Claude command");
    assert_eq!(
        cmd,
        "cd '/Users/x/brain' && claude --dangerously-skip-permissions --resume 'sess-9'"
    );
}

#[test]
fn prompt_is_appended_as_a_quoted_arg() {
    let cmd = build_llm_command(
        &PathBuf::from("/Users/x/brain"),
        AgentKind::Claude,
        "claude",
        &Plan::Fresh("uuid-1".to_owned()),
        Some("Defer T123 by 7 days"),
    )
    .expect("Claude prompt command");
    assert!(cmd.ends_with("'Defer T123 by 7 days'"));
}

#[test]
fn empty_prompt_adds_no_trailing_arg() {
    let cmd = build_llm_command(
        &PathBuf::from("/Users/x/brain"),
        AgentKind::Claude,
        "claude",
        &Plan::Resume("sess-9".to_owned()),
        Some("   "),
    )
    .expect("Claude empty-prompt command");
    assert!(cmd.ends_with("--resume 'sess-9'"));
    assert!(!cmd.contains("''"));
}

#[test]
fn blank_legacy_session_ids_return_the_typed_validation_error() {
    for plan in [Plan::Fresh("   ".to_owned()), Plan::Resume(String::new())] {
        assert_eq!(
            build_llm_command(
                &PathBuf::from("/Users/x/brain"),
                AgentKind::Claude,
                "claude",
                &plan,
                None,
            ),
            Err(crate::agent::AgentError::EmptySessionId)
        );
    }
}

#[test]
fn prompt_with_a_single_quote_is_escaped() {
    let cmd = build_llm_command(
        &PathBuf::from("/Users/x/brain"),
        AgentKind::Claude,
        "claude",
        &Plan::Fresh("u".to_owned()),
        Some("don't break"),
    )
    .expect("Claude quoted-prompt command");
    assert!(cmd.contains(r"'don'\''t break'"));
}

#[test]
fn codex_resume_uses_resume_subcommand() {
    let cmd = build_llm_command(
        &PathBuf::from("/Users/x/brain"),
        AgentKind::Codex,
        "codex",
        &Plan::Resume("sess-9".to_owned()),
        None,
    )
    .expect("Codex resume command");
    assert_eq!(
        cmd,
        "cd '/Users/x/brain' && codex --dangerously-bypass-hook-trust resume 'sess-9'"
    );
}

#[test]
fn codex_fresh_uses_configured_base_command_without_claude_flags() {
    let cmd = build_llm_command(
        &PathBuf::from("/Users/x/brain"),
        AgentKind::Codex,
        "codex --model gpt-5",
        &Plan::Fresh("uuid-1".to_owned()),
        Some("Start here"),
    )
    .expect("Codex fresh command");
    assert_eq!(
        cmd,
        "cd '/Users/x/brain' && codex --model gpt-5 --dangerously-bypass-hook-trust -- 'Start here'"
    );
    assert!(!cmd.contains("--session-id"));
    assert!(!cmd.contains("--resume"));
}

#[test]
fn launch_matrix_preserves_cwd_prefix_and_frontend_specific_session_syntax() {
    let root = PathBuf::from("/workspaces/family brain");
    let prompt = Some("  don't lose this  ");

    let cases = [
        (
            AgentKind::Claude,
            " claude --model sonnet ",
            Plan::Fresh("fresh-1".to_owned()),
            "cd '/workspaces/family brain' && claude --model sonnet --session-id 'fresh-1' -- 'don'\\''t lose this'",
        ),
        (
            AgentKind::Claude,
            " claude --model sonnet ",
            Plan::Resume("resume-1".to_owned()),
            "cd '/workspaces/family brain' && claude --model sonnet --resume 'resume-1' -- 'don'\\''t lose this'",
        ),
        (
            AgentKind::Codex,
            " codex --model gpt-5 ",
            Plan::Fresh("fresh-1".to_owned()),
            "cd '/workspaces/family brain' && codex --model gpt-5 --dangerously-bypass-hook-trust -- 'don'\\''t lose this'",
        ),
        (
            AgentKind::Codex,
            " codex --model gpt-5 ",
            Plan::Resume("resume-1".to_owned()),
            "cd '/workspaces/family brain' && codex --model gpt-5 --dangerously-bypass-hook-trust resume 'resume-1' -- 'don'\\''t lose this'",
        ),
    ];

    for (agent, configured_command, plan, expected) in cases {
        assert_eq!(
            build_llm_command(&root, agent, configured_command, &plan, prompt),
            Ok(expected.to_owned())
        );
    }
}

#[test]
fn project_dir_name_mangles_slashes_to_dashes() {
    assert_eq!(
        project_dir_name(&PathBuf::from("/Users/x/brain")),
        "-Users-x-brain"
    );
    // Dots are mangled too (claude's convention).
    assert_eq!(
        project_dir_name(&PathBuf::from("/Users/x/.brain")),
        "-Users-x--brain"
    );
}

#[test]
fn env_carries_instance_pid_and_db_path() {
    let env = env_for(
        &workspace(),
        &actor(),
        AgentKind::Claude,
        "inst-1",
        4321,
        &PathBuf::from("/tmp/state.db"),
        "response-1",
    );
    assert!(env.contains(&("BRAIN_INSTANCE_ID".to_owned(), "inst-1".to_owned())));
    assert!(env.contains(&("BRAIN_PID".to_owned(), "4321".to_owned())));
    assert!(env.contains(&("BRAIN_STATE_DB".to_owned(), "/tmp/state.db".to_owned())));
    assert!(env.contains(&("BRAIN_ACTOR_ID".to_owned(), "pablo".to_owned())));
    assert!(env.contains(&("BRAIN_CHANNEL".to_owned(), "interactive".to_owned())));
    assert!(env.contains(&("BRAIN_AGENT_KIND".to_owned(), "claude".to_owned())));
    assert!(env.contains(&("BRAIN_RESPONSE_ID".to_owned(), "response-1".to_owned())));
}

#[test]
fn skill_session_env_carries_done_url_and_token() {
    let env = env_for_skill_session(
        &workspace(),
        &actor(),
        AgentKind::Claude,
        "http://127.0.0.1:8787/session/done",
        "tok-9",
    );
    assert!(env.contains(&(
        "BRAIN_SESSION_DONE_URL".to_owned(),
        "http://127.0.0.1:8787/session/done".to_owned()
    )));
    assert!(env.contains(&("BRAIN_SESSION_TOKEN".to_owned(), "tok-9".to_owned())));
}

#[test]
fn skill_session_env_omits_the_tracking_vars_so_the_session_stays_ephemeral() {
    // The SessionStart hook keys off BRAIN_INSTANCE_ID / BRAIN_STATE_DB;
    // their absence is exactly what keeps a skill session out of the DB.
    let env = env_for_skill_session(
        &workspace(),
        &actor(),
        AgentKind::Claude,
        "http://127.0.0.1:8787/session/done",
        "tok-9",
    );
    let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
    assert!(!keys.contains(&"BRAIN_INSTANCE_ID"));
    assert!(!keys.contains(&"BRAIN_STATE_DB"));
}

#[test]
fn skill_session_env_stays_ephemeral_for_each_frontend() {
    for (agent, expected_kind) in [(AgentKind::Claude, "claude"), (AgentKind::Codex, "codex")] {
        let env = env_for_skill_session(
            &workspace(),
            &actor(),
            agent,
            "http://127.0.0.1:8787/session/done",
            "tok-9",
        );
        let keys: Vec<&str> = env.iter().map(|(key, _)| key.as_str()).collect();

        assert!(env.contains(&("BRAIN_AGENT_KIND".to_owned(), expected_kind.to_owned())));
        assert!(env.contains(&("BRAIN_SESSION_TOKEN".to_owned(), "tok-9".to_owned())));
        assert!(!keys.contains(&"BRAIN_INSTANCE_ID"));
        assert!(!keys.contains(&"BRAIN_STATE_DB"));
    }
}
