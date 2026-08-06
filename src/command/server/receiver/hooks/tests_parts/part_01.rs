#[test]
fn command_is_project_relative_for_paths_under_the_selected_root() {
    let command = command(
        Path::new("/Users/pablo/family/.claude/brain-hooks/claude_stop_hook.py"),
        Path::new("/Users/pablo/family"),
    );
    assert_eq!(command, "python3 .claude/brain-hooks/claude_stop_hook.py");
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
        Path::new("/Users/pablo/family/.claude/brain-hooks/claude_stop_hook.py"),
        Path::new("/Users/pablo/family"),
    );
    let mbp = command(
        Path::new("/Users/member-b/fam-brain/.claude/brain-hooks/claude_stop_hook.py"),
        Path::new("/Users/member-b/fam-brain"),
    );
    assert_eq!(mini, mbp);
}

#[test]
fn codex_command_uses_portable_brain_root_and_is_cwd_independent() {
    let command = codex_command(Path::new(
        "/Users/pablo/family/.claude/brain-hooks/claude_stop_hook.py",
    ));
    assert_eq!(
        command,
        r#"python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/claude_stop_hook.py""#
    );
}
