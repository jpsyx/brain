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

/// A hook command runs wherever the agent last changed directory to, which is
/// somewhere the agent chooses and brain cannot predict. Any frontend whose
/// command is relative silently stops firing the moment an agent runs `cd`, and
/// a completion hook that stops firing means a message never gets its reply.
/// This held for Claude and had to hold for every frontend, so it is asserted
/// over the whole registry rather than per adapter.
#[test]
fn no_frontend_registers_a_working_directory_relative_hook_command() {
    let mut checked = 0;
    for installation in lifecycle_installations() {
        let crate::agent::LifecyclePayload::HookSettings {
            style,
            session_script,
            completion_script,
            ..
        } = installation.payload()
        else {
            continue;
        };
        for script in [session_script, completion_script] {
            let command = match style {
                crate::agent::HookCommandStyle::ClaudeProjectDir => {
                    claude_project_dir_command(Path::new(script))
                }
                crate::agent::HookCommandStyle::PortableBrainRoot => {
                    portable_root_command(Path::new(script))
                }
            };
            assert!(
                command.contains("${"),
                "{} resolves {script} against the working directory: {command}",
                installation.id()
            );
            assert!(
                !command.contains("python3 ."),
                "{} uses a relative path: {command}",
                installation.id()
            );
            checked += 1;
        }
    }
    assert!(checked >= 2, "no hook commands were checked at all");
}
