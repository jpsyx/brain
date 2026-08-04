use super::*;
use crate::agent::{AgentTransport, HookMetadata, LaunchSpec};
use std::time::Duration;

fn spec(command: &str, cwd: &Path) -> LaunchSpec {
    LaunchSpec::new(command, cwd.to_path_buf(), Vec::new(), HookMetadata::none())
}

fn wait_until_stopped(pty: &PtyPane) {
    for _ in 0..300 {
        if !AgentTransport::is_alive(pty) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("PTY child did not stop");
}

fn wait_for_file(path: &Path) {
    for _ in 0..300 {
        if path.is_file() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("PTY child did not create {}", path.display());
}

/// Run a command in a small PTY and block until the child exits and its
/// output has been parsed into the vt100 screen + scrollback.
fn run_and_settle(command: &str, rows: u16, cols: u16) -> PtyPane {
    run_and_settle_with_env(command, &[], rows, cols)
}

fn run_and_settle_with_env(
    command: &str,
    environment: &[(String, String)],
    rows: u16,
    cols: u16,
) -> PtyPane {
    let pty =
        PtyPane::spawn_shell_command_with_env(command, environment, Path::new("."), rows, cols)
            .expect("spawn pty");
    for _ in 0..300 {
        if !pty.is_alive() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    // Let the reader thread drain the final bytes after EOF.
    thread::sleep(Duration::from_millis(80));
    pty
}

#[test]
fn transport_does_not_inherit_unrelated_workspace_secrets() {
    const CHILD_MARKER: &str = "BRAIN_PTY_ENV_LEAK_CHILD";
    const SECRET: &str = "OTHER_WORKSPACE_SECRET";
    if std::env::var_os(CHILD_MARKER).is_some() {
        let pty = run_and_settle(
            "printf '%s|%s' \"${OTHER_WORKSPACE_SECRET-absent}\" \
             \"${BRAIN_PTY_ENV_LEAK_CHILD-absent}\"",
            5,
            80,
        );
        assert!(pty.contents().contains("absent|absent"));
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "pty_pane::tests::transport_does_not_inherit_unrelated_workspace_secrets",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD_MARKER, "must-not-reach-agent")
        .env(SECRET, "another-workspace-token")
        .output()
        .expect("spawn isolated test child");

    assert!(
        output.status.success(),
        "isolated child failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn transport_does_not_rehydrate_unrelated_secrets_from_shell_profiles() {
    const CHILD_MARKER: &str = "BRAIN_PTY_PROFILE_LEAK_CHILD";
    const SECRET: &str = "OTHER_WORKSPACE_PROFILE_SECRET";
    if std::env::var_os(CHILD_MARKER).is_some() {
        let home = tempfile::tempdir().expect("isolated profile home");
        let shell = std::env::var("SHELL").expect("injected test shell");
        let profile = match Path::new(&shell).file_name().and_then(|name| name.to_str()) {
            Some("zsh") => ".zshrc",
            Some("bash") => ".bashrc",
            other => panic!("unsupported profile test shell: {other:?}"),
        };
        std::fs::write(
            home.path().join(profile),
            format!("export {SECRET}=profile-rehydrated\n"),
        )
        .expect("write isolated shell profile");
        std::fs::write(
            home.path().join(".profile"),
            format!("export {SECRET}=profile-rehydrated\n"),
        )
        .expect("write isolated POSIX login profile");
        let environment = vec![
            ("HOME".to_owned(), home.path().display().to_string()),
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        ];

        let pty = run_and_settle_with_env(
            "printf '%s' \"${OTHER_WORKSPACE_PROFILE_SECRET-absent}\"",
            &environment,
            5,
            80,
        );
        assert!(pty.contents().contains("absent"));
        assert!(!pty.contents().contains("profile-rehydrated"));
        return;
    }

    let shell = ["/bin/zsh", "/bin/bash"]
        .into_iter()
        .find(|candidate| Path::new(candidate).is_file())
        .expect("profile-loading test shell");
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "pty_pane::tests::transport_does_not_rehydrate_unrelated_secrets_from_shell_profiles",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD_MARKER, "1")
        .env("SHELL", shell)
        .output()
        .expect("spawn isolated profile test child");

    assert!(
        output.status.success(),
        "isolated child failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn non_login_sh_does_not_source_home_profile() {
    let home = tempfile::tempdir().expect("isolated profile home");
    std::fs::write(
        home.path().join(".profile"),
        "export OTHER_WORKSPACE_PROFILE_SECRET=profile-rehydrated\n",
    )
    .expect("write isolated profile");
    let run = |mode: &str| {
        std::process::Command::new("/bin/sh")
            .args([
                mode,
                "printf '%s' \"${OTHER_WORKSPACE_PROFILE_SECRET-absent}\"",
            ])
            .env_clear()
            .env("HOME", home.path())
            .env("PATH", "/usr/bin:/bin")
            .output()
            .expect("run isolated /bin/sh")
    };

    let login = run("-lc");
    let non_login = run("-c");
    eprintln!(
        "/bin/sh -lc: {:?}; /bin/sh -c: {:?}",
        String::from_utf8_lossy(&login.stdout),
        String::from_utf8_lossy(&non_login.stdout)
    );
    assert!(login.status.success());
    assert!(non_login.status.success());
    assert_eq!(login.stdout, b"profile-rehydrated");
    assert_eq!(non_login.stdout, b"absent");
}

#[test]
fn transport_spawns_from_the_complete_launch_spec() {
    let directory = tempfile::tempdir().expect("temporary cwd");
    let spec = LaunchSpec::new(
        "printf '%s\\n' \"$PWD\"; printf '%s' \"$BRAIN_TRANSPORT_MARKER\"",
        directory.path().to_path_buf(),
        vec![(
            "BRAIN_TRANSPORT_MARKER".to_owned(),
            "launch-spec-env".to_owned(),
        )],
        HookMetadata::none(),
    );
    let mut pty = PtyPane::new(5, 80);

    AgentTransport::spawn(&mut pty, &spec).expect("spawn through transport");
    for _ in 0..300 {
        if !AgentTransport::is_alive(&pty) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    thread::sleep(Duration::from_millis(80));

    let output = AgentTransport::snapshot(&pty);
    assert!(output.contains(&directory.path().display().to_string()));
    assert!(output.contains("launch-spec-env"));
}

#[test]
fn dormant_transport_has_inert_lifecycle_and_rejects_input() {
    let mut pty = PtyPane::new(5, 80);

    assert!(!AgentTransport::is_alive(&pty));
    assert_eq!(AgentTransport::snapshot(&pty), "");
    assert_eq!(
        AgentTransport::send(&mut pty, InputSequence::bytes(b"ignored")),
        Err(AgentError::Transport("PTY child is not running".to_owned()))
    );
    AgentTransport::shutdown(&mut pty);
    AgentTransport::shutdown(&mut pty);
    assert!(!AgentTransport::is_alive(&pty));
}

#[test]
fn transport_rejects_input_after_the_child_exits() {
    let mut pty = PtyPane::new(5, 80);
    AgentTransport::spawn(&mut pty, &spec("true", Path::new("."))).expect("spawn child");
    wait_until_stopped(&pty);

    assert_eq!(
        AgentTransport::send(&mut pty, InputSequence::bytes(b"too late")),
        Err(AgentError::Transport("PTY child is not running".to_owned()))
    );
}

#[test]
fn transport_rejects_a_second_spawn_while_the_child_is_alive() {
    let mut pty = PtyPane::new(5, 80);
    let running = spec("sleep 30", Path::new("."));
    AgentTransport::spawn(&mut pty, &running).expect("spawn child");

    assert_eq!(
        AgentTransport::spawn(&mut pty, &spec("true", Path::new("."))),
        Err(AgentError::Transport(
            "cannot replace a running PTY child".to_owned()
        ))
    );
    AgentTransport::shutdown(&mut pty);
    wait_until_stopped(&pty);
}

#[test]
fn spawn_failure_leaves_the_transport_dormant_and_reusable() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut pty = PtyPane::new(5, 80);
    let missing_shell = temporary.path().join("missing-shell");

    assert!(pty.start(CommandBuilder::new(missing_shell)).is_err());
    assert!(!AgentTransport::is_alive(&pty));
    assert_eq!(
        AgentTransport::send(&mut pty, InputSequence::bytes(b"ignored")),
        Err(AgentError::Transport("PTY child is not running".to_owned()))
    );

    AgentTransport::spawn(&mut pty, &spec("true", temporary.path())).expect("spawn after failure");
    wait_until_stopped(&pty);
}

#[test]
fn shutdown_stops_the_child_and_rejects_later_input() {
    let mut pty = PtyPane::new(5, 80);
    AgentTransport::spawn(&mut pty, &spec("sleep 30", Path::new("."))).expect("spawn child");

    AgentTransport::shutdown(&mut pty);
    wait_until_stopped(&pty);
    assert_eq!(
        AgentTransport::send(&mut pty, InputSequence::bytes(b"too late")),
        Err(AgentError::Transport("PTY child is not running".to_owned()))
    );
}

#[test]
fn dropping_the_transport_stops_its_child() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let heartbeat = temporary.path().join("heartbeat");
    let command = format!(
        "while :; do printf x >> {}; sleep 0.02; done",
        crate::agent::frontend::shell_quote(&heartbeat.display().to_string())
    );
    let mut pty = PtyPane::new(5, 80);
    AgentTransport::spawn(&mut pty, &spec(&command, temporary.path())).expect("spawn child");
    wait_for_file(&heartbeat);

    drop(pty);
    thread::sleep(Duration::from_millis(100));
    let settled_size = std::fs::metadata(&heartbeat)
        .expect("heartbeat metadata")
        .len();
    thread::sleep(Duration::from_millis(200));
    let final_size = std::fs::metadata(&heartbeat)
        .expect("heartbeat metadata")
        .len();
    assert_eq!(
        final_size, settled_size,
        "PTY child remained active after drop"
    );
}

#[test]
fn scroll_up_enters_scrollback_and_scroll_down_returns() {
    // 200 lines into a 5-row terminal pushes ~195 rows into scrollback.
    let pty = run_and_settle("seq 1 200", 5, 20);
    assert_eq!(pty.scrollback_offset(), 0, "starts pinned to the live tail");

    pty.scroll_up(10);
    assert_eq!(pty.scrollback_offset(), 10);

    pty.scroll_down(4);
    assert_eq!(pty.scrollback_offset(), 6);

    // Over-scrolling down saturates at the live tail (0).
    pty.scroll_down(1000);
    assert_eq!(pty.scrollback_offset(), 0);
}

#[test]
fn scroll_up_is_clamped_to_available_scrollback() {
    let pty = run_and_settle("seq 1 200", 5, 20);
    // Asking for far more than exists clamps to the real scrollback
    // length rather than running off the end.
    pty.scroll_up(1_000_000);
    let max = pty.scrollback_offset();
    assert!(max > 0, "should have real scrollback to enter");
    // Asking again past the top is idempotent (already clamped).
    pty.scroll_up(1_000_000);
    assert_eq!(pty.scrollback_offset(), max);
}
