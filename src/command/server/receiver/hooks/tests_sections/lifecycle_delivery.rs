#[test]
fn concurrent_workspace_installs_keep_every_codex_config_inside_its_root() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let family = temp.path().join("family");
    let work = temp.path().join("work");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::create_dir_all(&family).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(
        home.join(".codex/hooks.json"),
        serde_json::to_vec(&json!({"permissions": {"allow": ["Read"]}})).unwrap(),
    )
    .unwrap();

    std::thread::scope(|scope| {
        for root in [&family, &work] {
            let home = home.clone();
            scope.spawn(move || install_for_home(root, &home).unwrap());
        }
    });

    for root in [&family, &work] {
        assert_eq!(
            configured_command(&root.join(".claude/settings.json"), "SessionStart"),
            r#"test -z "${BRAIN_ROOT-}" || python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/agent_session_start_hook.py""#
        );
        assert_eq!(
            configured_command(&root.join(".claude/settings.json"), "Stop"),
            r#"test -z "${BRAIN_ROOT-}" || python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/agent_session_stop_hook.py""#
        );
        assert_eq!(
            configured_command(&root.join(".claude/settings.json"), "UserPromptSubmit"),
            r#"test -z "${BRAIN_ROOT-}" || python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/receiver_observation_bridge.py""#
        );
        assert_eq!(
            configured_command(&root.join(".claude/settings.json"), "PostToolUse"),
            r#"test -z "${BRAIN_ROOT-}" || python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/receiver_observation_bridge.py""#
        );
        assert_eq!(
            configured_command(&root.join(".codex/hooks.json"), "SessionStart"),
            r#"test -z "${BRAIN_ROOT-}" || python3 "${BRAIN_ROOT}/.brain/hooks/agent_session_start_hook.py""#
        );
        assert_eq!(
            configured_command(&root.join(".codex/hooks.json"), "Stop"),
            r#"test -z "${BRAIN_ROOT-}" || python3 "${BRAIN_ROOT}/.brain/hooks/agent_session_stop_hook.py""#
        );
        assert_eq!(
            configured_command(&root.join(".codex/hooks.json"), "UserPromptSubmit"),
            r#"test -z "${BRAIN_ROOT-}" || python3 "${BRAIN_ROOT}/.brain/hooks/receiver_observation_bridge.py""#
        );
        assert_eq!(
            configured_command(&root.join(".codex/hooks.json"), "PostToolUse"),
            r#"test -z "${BRAIN_ROOT-}" || python3 "${BRAIN_ROOT}/.brain/hooks/receiver_observation_bridge.py""#
        );
        assert!(root.join(".brain/hooks/receiver_observation_bridge.py").is_file());
    }
    let codex_bytes = std::fs::read(home.join(".codex/hooks.json")).unwrap();
    let codex: serde_json::Value = serde_json::from_slice(&codex_bytes).unwrap();
    assert_eq!(codex["permissions"]["allow"][0], "Read");
    assert!(codex.get("hooks").is_none());
}

#[test]
fn stop_hook_recovers_final_message_from_transcript_when_field_absent() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join("brain");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    install_for_home(&root, &home).unwrap();
    let settings = root.join(".claude/settings.json");
    let start = configured_command(&settings, "SessionStart");
    let stop = configured_command(&settings, "Stop");

    let db_path = temp.path().join("state.db");
    drop(crate::state::Db::open_path(&db_path).unwrap());
    let connection = rusqlite::Connection::open(&db_path).unwrap();
    connection
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES ('claude', 'pending-claude-launch', 'instance-1', 4242, 'test-launch',
                     '11111111-1111-4111-8111-111111111111',
                     'pablo', 'sms', 1, 1)",
            [],
        )
        .unwrap();
    drop(connection);

    // A realistic Claude Code transcript: the turn ends on a text message,
    // preceded by a thinking-only assistant message. Claude Code sends
    // `last_assistant_message` today, but the hook must not depend on it — an
    // absent field is the failure mode that silently dropped every SMS reply.
    let transcript = temp.path().join("session.jsonl");
    std::fs::write(
        &transcript,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"question"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"the final answer"}]}}"#,
            "\n",
        ),
    )
    .unwrap();

    let response_dir = temp.path().join("responses");
    let common = [
        (
            "BRAIN_WORKSPACE_ID",
            std::ffi::OsStr::new("11111111-1111-4111-8111-111111111111"),
        ),
        ("BRAIN_WORKSPACE", std::ffi::OsStr::new("brain")),
        ("BRAIN_ROOT", root.as_os_str()),
        ("BRAIN_ACTOR_ID", std::ffi::OsStr::new("pablo")),
        ("BRAIN_CHANNEL", std::ffi::OsStr::new("sms")),
        ("BRAIN_AGENT_KIND", std::ffi::OsStr::new("claude")),
        ("BRAIN_INSTANCE_ID", std::ffi::OsStr::new("instance-1")),
        ("BRAIN_PID", std::ffi::OsStr::new("4242")),
        ("BRAIN_STATE_DB", db_path.as_os_str()),
        ("BRAIN_RESPONSE_DIR", response_dir.as_os_str()),
        (
            "BRAIN_RESPONSE_ID",
            std::ffi::OsStr::new("response-claude-1"),
        ),
    ];

    let started = run_configured(
        &root,
        &start,
        &common,
        &json!({"session_id":"claude-session-1","source":"startup"}),
    );
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );

    let stopped = run_configured(
        &root,
        &stop,
        &common,
        &json!({
            "session_id":"claude-session-1",
            "transcript_path": transcript.to_string_lossy(),
            "hook_event_name":"Stop",
            "stop_hook_active": false
        }),
    );
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );

    let response: serde_json::Value = serde_json::from_slice(
        &std::fs::read(response_dir.join("response-claude-1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(response["message"], "the final answer");
    assert_eq!(response["session_id"], "claude-session-1");
    assert_eq!(response["channel"], "sms");
}

#[test]
fn installed_codex_start_and_stop_hooks_complete_one_attributed_lifecycle() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join("brain");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let stale_dir = root.join(".brain/hooks");
    std::fs::create_dir_all(&stale_dir).unwrap();
    std::fs::write(stale_dir.join("claude_session_start_hook.py"), "# stale\n").unwrap();
    std::fs::write(stale_dir.join("claude_stop_hook.py"), "# stale\n").unwrap();
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(
        root.join(".claude/settings.json"),
        serde_json::to_vec(&serde_json::json!({
            "hooks": {
                "SessionStart": [{"hooks": [{"type": "command", "command": r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/claude_session_start_hook.py""#}]}],
                "Stop": [{"hooks": [{"type": "command", "command": r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/claude_stop_hook.py""#}] }]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(root.join(".codex")).unwrap();
    std::fs::write(
        root.join(".codex/hooks.json"),
        serde_json::to_vec(&serde_json::json!({
            "hooks": {
                "SessionStart": [{"hooks": [{"type": "command", "command": r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/claude_session_start_hook.py""#}]}],
                "Stop": [{"hooks": [{"type": "command", "command": r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/claude_stop_hook.py""#}] }]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    install_for_home(&root, &home).unwrap();
    let codex_hooks = root.join(".codex/hooks.json");
    let codex_schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&codex_hooks).unwrap()).unwrap();
    assert_eq!(
        codex_schema["hooks"]["SessionStart"],
        json!([{"hooks": [{
            "type": "command",
            "command": "test -z \"${BRAIN_ROOT-}\" || python3 \"${BRAIN_ROOT}/.brain/hooks/agent_session_start_hook.py\""
        }]}])
    );
    assert_eq!(
        codex_schema["hooks"]["Stop"],
        json!([{"hooks": [{
            "type": "command",
            "command": "test -z \"${BRAIN_ROOT-}\" || python3 \"${BRAIN_ROOT}/.brain/hooks/agent_session_stop_hook.py\""
        }]}])
    );
    let start = configured_command(&codex_hooks, "SessionStart");
    let stop = configured_command(&codex_hooks, "Stop");
    let db_path = temp.path().join("state.db");
    drop(crate::state::Db::open_path(&db_path).unwrap());
    let connection = rusqlite::Connection::open(&db_path).unwrap();
    connection
        .execute(
            "INSERT INTO brain_sessions
               (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
                workspace_id, actor_id, channel, created_at, last_active_at)
             VALUES ('codex', 'pending-codex-launch', 'instance-1', 4242, 'test-launch',
                     '11111111-1111-4111-8111-111111111111',
                     'pablo', 'interactive', 1, 1)",
            [],
        )
        .unwrap();
    let response_dir = temp.path().join("responses");
    let common = [
        (
            "BRAIN_WORKSPACE_ID",
            std::ffi::OsStr::new("11111111-1111-4111-8111-111111111111"),
        ),
        ("BRAIN_WORKSPACE", std::ffi::OsStr::new("brain")),
        ("BRAIN_ROOT", root.as_os_str()),
        ("BRAIN_ACTOR_ID", std::ffi::OsStr::new("pablo")),
        ("BRAIN_CHANNEL", std::ffi::OsStr::new("interactive")),
        ("BRAIN_AGENT_KIND", std::ffi::OsStr::new("codex")),
        ("BRAIN_INSTANCE_ID", std::ffi::OsStr::new("instance-1")),
        ("BRAIN_PID", std::ffi::OsStr::new("4242")),
        ("BRAIN_STATE_DB", db_path.as_os_str()),
        ("BRAIN_RESPONSE_DIR", response_dir.as_os_str()),
        ("BRAIN_RESPONSE_ID", std::ffi::OsStr::new("response-1")),
    ];

    let started = run_configured(
        &root,
        &start,
        &common,
        &serde_json::json!({"session_id":"codex-thread-1","source":"startup"}),
    );
    let stopped = run_configured(
        &root,
        &stop,
        &common,
        &serde_json::json!({
            "session_id":"codex-thread-1",
            "last_assistant_message":"Finished"
        }),
    );

    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stderr)
    );
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    let connection = rusqlite::Connection::open(db_path).unwrap();
    let attribution = connection
        .query_row(
            "SELECT agent_kind, actor_id, channel, completion_status FROM brain_sessions
             WHERE agent_session_id = 'codex-thread-1'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        attribution,
        (
            "codex".to_owned(),
            "pablo".to_owned(),
            "interactive".to_owned(),
            "completed".to_owned()
        )
    );
    let response: serde_json::Value =
        serde_json::from_slice(&std::fs::read(response_dir.join("response-1.json")).unwrap())
            .unwrap();
    assert_eq!(response["session_id"], "codex-thread-1");
    assert_eq!(response["frontend"], "codex");
    assert_eq!(
        response["workspace_id"],
        "11111111-1111-4111-8111-111111111111"
    );
    assert_eq!(response["actor_id"], "pablo");
    assert_eq!(response["channel"], "interactive");
}

/// Every machine already running Brain has the old working-directory-relative
/// command on disk. Reinstalling must *replace* it rather than leave it beside
/// the new one, or the broken command keeps firing and every turn runs the hook
/// twice.
#[test]
fn reinstalling_replaces_the_broken_relative_command_instead_of_duplicating_it() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join("brain");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(
        root.join(".claude/settings.json"),
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"python3 .brain/hooks/agent_session_start_hook.py"}]}],"Stop":[{"hooks":[{"type":"command","command":"python3 .brain/hooks/agent_session_stop_hook.py"}]}]},"permissions":{"allow":["Read"]}}"#,
    )
    .unwrap();

    install_for_home(&root, &home).unwrap();

    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(".claude/settings.json")).unwrap())
            .unwrap();
    for event in ["SessionStart", "Stop"] {
        let entries = settings["hooks"][event].as_array().unwrap();
        assert_eq!(entries.len(), 1, "{event} kept a duplicate hook entry");
        let command = entries[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(
            command.contains("CLAUDE_PROJECT_DIR"),
            "{event} kept the broken relative command: {command}"
        );
    }
    assert_eq!(
        settings["permissions"]["allow"][0], "Read",
        "unrelated settings must survive the repair"
    );
}

#[test]
fn reinstalling_replaces_unguarded_codex_commands_instead_of_duplicating_them() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let root = temp.path().join("brain");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(root.join(".codex")).unwrap();
    std::fs::write(
        root.join(".codex/hooks.json"),
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"python3 \"${BRAIN_ROOT}/.brain/hooks/agent_session_start_hook.py\""}]}],"Stop":[{"hooks":[{"type":"command","command":"python3 \"${BRAIN_ROOT}/.brain/hooks/agent_session_stop_hook.py\""}]}],"UserPromptSubmit":[{"hooks":[{"type":"command","command":"python3 \"${BRAIN_ROOT}/.brain/hooks/receiver_observation_bridge.py\""}]}],"PostToolUse":[{"hooks":[{"type":"command","command":"python3 \"${BRAIN_ROOT}/.brain/hooks/receiver_observation_bridge.py\""}]}]}}"#,
    )
    .unwrap();

    install_for_home(&root, &home).unwrap();

    let hooks = root.join(".codex/hooks.json");
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&hooks).unwrap()).unwrap();
    for (event, script) in [
        ("SessionStart", "agent_session_start_hook.py"),
        ("Stop", "agent_session_stop_hook.py"),
        ("UserPromptSubmit", "receiver_observation_bridge.py"),
        ("PostToolUse", "receiver_observation_bridge.py"),
    ] {
        assert_eq!(
            configured_command(&hooks, event),
            format!(
                r#"test -z "${{BRAIN_ROOT-}}" || python3 "${{BRAIN_ROOT}}/.brain/hooks/{script}""#
            )
        );
        assert_eq!(
            settings["hooks"][event].as_array().unwrap().len(),
            1,
            "{event} retained the unguarded command"
        );
    }
}
