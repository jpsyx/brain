/// Claude runs a hook in the session's *current* working directory, not the
/// project root, and its Bash tool's `cd` persists for the rest of the session.
/// A project-relative command therefore stopped resolving the moment an agent
/// changed directory: the turn-complete hook failed, no completion artifact was
/// written, and the message it was answering never got a reply. The command must
/// not depend on the working directory at all.
#[test]
fn claude_command_is_cwd_independent_because_hooks_run_wherever_the_agent_left_off() {
    let command =
        claude_project_dir_command(Path::new(".claude/brain-hooks/agent_turn_complete_hook.py"));
    assert_eq!(
        command,
        r#"python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/agent_turn_complete_hook.py""#
    );
    assert!(
        !command.contains("python3 .claude"),
        "a bare relative path resolves against whatever directory the agent last cd'd into"
    );
}

/// The settings file lives inside the synced workspace, so it must not carry a
/// machine-specific absolute path: the same file is read on every machine.
#[test]
fn claude_command_stays_identical_across_machines_and_workspace_roots() {
    let script = Path::new(".claude/brain-hooks/agent_turn_complete_hook.py");
    assert_eq!(
        claude_project_dir_command(script),
        claude_project_dir_command(script),
        "the command names no root, so every machine writes the same one"
    );
    assert!(!claude_project_dir_command(script).contains("/Users/"));
}

#[test]
fn portable_root_command_uses_brain_root_and_is_cwd_independent() {
    let command = portable_root_command(Path::new(
        ".claude/brain-hooks/agent_turn_complete_hook.py",
    ));
    assert_eq!(
        command,
        r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_turn_complete_hook.py""#
    );
}

#[test]
fn lifecycle_installations_follow_the_complete_frontend_registry() {
    assert_eq!(
        lifecycle_installations()
            .iter()
            .map(|installation| installation.id())
            .collect::<Vec<_>>(),
        vec![
            "agent-session-start-script",
            "agent-turn-complete-script",
            "claude-session-start-compatibility-script",
            "claude-stop-compatibility-script",
            "claude-settings",
            "codex-settings",
            "opencode-plugin",
        ]
    );
}
