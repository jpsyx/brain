#[test]
fn command_is_project_relative_for_paths_under_the_selected_root() {
    let command = command(
        Path::new("/Users/pablo/family/.claude/brain-hooks/agent_turn_complete_hook.py"),
        Path::new("/Users/pablo/family"),
    );
    assert_eq!(command, "python3 .claude/brain-hooks/agent_turn_complete_hook.py");
}

#[test]
fn command_falls_back_to_absolute_outside_the_selected_root() {
    assert_eq!(
        command(
            Path::new("/opt/hooks/x.py"),
            Path::new("/Users/pablo/family")
        ),
        "python3 /opt/hooks/x.py"
    );
}

#[test]
fn project_relative_command_is_identical_across_workspace_roots() {
    let mini = command(
        Path::new("/Users/pablo/family/.claude/brain-hooks/agent_turn_complete_hook.py"),
        Path::new("/Users/pablo/family"),
    );
    let mbp = command(
        Path::new("/Users/member-b/fam-brain/.claude/brain-hooks/agent_turn_complete_hook.py"),
        Path::new("/Users/member-b/fam-brain"),
    );
    assert_eq!(mini, mbp);
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
