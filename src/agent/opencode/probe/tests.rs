use std::{
    os::unix::{fs::PermissionsExt, process::CommandExt as _},
    process::Command,
    sync::{Barrier, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::agent::frontend::shell_quote;

use super::*;

struct FixtureRunner {
    outputs: Mutex<Vec<ProbeOutput>>,
}

impl FixtureRunner {
    fn new(help: &str, session_help: &str) -> Self {
        Self::with_version_and_debug(
            "1.18.14\n",
            help,
            session_help,
            "--pure run without external plugins\n",
        )
    }

    fn with_version(version: &str, help: &str, session_help: &str) -> Self {
        Self::with_version_and_debug(
            version,
            help,
            session_help,
            "--pure run without external plugins\n",
        )
    }

    fn with_version_and_debug(
        version: &str,
        help: &str,
        session_help: &str,
        debug_help: &str,
    ) -> Self {
        Self {
            outputs: Mutex::new(vec![
                ProbeOutput::success(version),
                ProbeOutput::success(help),
                ProbeOutput::success(session_help),
                ProbeOutput::success(debug_help),
                ProbeOutput::success("{}\n"),
                ProbeOutput::success("{}\n"),
            ]),
        }
    }

    fn compatible_without_agent() -> Self {
        Self::new(
            "--prompt <text>\n--session <id>\n",
            "--format <format> json\n",
        )
    }
}

impl ProbeRunner for FixtureRunner {
    fn run(&self, _command: &str, _arguments: &[&str]) -> Result<ProbeOutput, ProbeRunError> {
        Ok(self.outputs.lock().expect("fixture outputs").remove(0))
    }
}

#[derive(Default)]
struct RecordingRunner {
    calls: Mutex<Vec<(String, Vec<String>)>>,
}

impl ProbeRunner for RecordingRunner {
    fn run(&self, command: &str, arguments: &[&str]) -> Result<ProbeOutput, ProbeRunError> {
        self.calls.lock().expect("recorded calls").push((
            command.to_owned(),
            arguments
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ));
        let output = match arguments {
            ["--version"] => "1.18.14\n",
            ["--help"] => "--agent <name>\n--prompt <text>\n--session <id>\n",
            ["session", "list", "--help"] => "--format <format> json\n",
            ["debug", "config", "--help"] => "--pure run without external plugins\n",
            ["debug", "config", "--pure"] | ["debug", "config"] => "{}\n",
            _ => return Err(ProbeRunError::Start),
        };
        Ok(ProbeOutput::success(output))
    }
}

#[test]
fn rejects_help_without_the_agent_flag_and_redacts_the_command() {
    let runner = FixtureRunner::compatible_without_agent();
    let command = "private-wrapper --token local-secret opencode";

    let error = inspect_with(command, &runner).expect_err("missing --agent");
    let message = error.to_string();

    assert_eq!(
        message,
        "frontend error: OpenCode is incompatible: missing TUI capability `--agent`. Update OpenCode or set `brain env set opencode_cmd <command>` to a compatible command."
    );
    assert!(!message.contains("private-wrapper"));
    assert!(!message.contains("local-secret"));
}

#[test]
fn rejects_help_without_the_prompt_flag() {
    let runner = FixtureRunner::new(
        "--agent <name>\n--session <id>\n",
        "--format <format> json\n",
    );

    let error = inspect_with("opencode", &runner).expect_err("missing --prompt");

    assert_eq!(
        error.to_string(),
        "frontend error: OpenCode is incompatible: missing TUI capability `--prompt`. Update OpenCode or set `brain env set opencode_cmd <command>` to a compatible command."
    );
}

#[test]
fn rejects_help_without_the_session_flag() {
    let runner = FixtureRunner::new(
        "--agent <name>\n--prompt <text>\n",
        "--format <format> json\n",
    );

    let error = inspect_with("opencode", &runner).expect_err("missing --session");

    assert_eq!(
        error.to_string(),
        "frontend error: OpenCode is incompatible: missing TUI capability `--session`. Update OpenCode or set `brain env set opencode_cmd <command>` to a compatible command."
    );
}

#[test]
fn rejects_session_list_help_without_json_format_support() {
    let runner = FixtureRunner::new(
        "--agent <name>\n--prompt <text>\n--session <id>\n",
        "List sessions\n",
    );

    let error = inspect_with("opencode", &runner).expect_err("missing JSON session list");

    assert_eq!(
        error.to_string(),
        "frontend error: OpenCode is incompatible: missing session-list capability `--format json`. Update OpenCode or set `brain env set opencode_cmd <command>` to a compatible command."
    );
}

#[test]
fn rejects_debug_config_without_the_pure_schema_probe_surface() {
    let runner = FixtureRunner::with_version_and_debug(
        "1.18.14\n",
        "--agent <name>\n--prompt <text>\n--session <id>\n",
        "--format <format> json\n",
        "show resolved configuration\n",
    );

    let error = inspect_with("opencode", &runner).expect_err("missing pure config probe");

    assert_eq!(
        error.to_string(),
        "frontend error: OpenCode is incompatible: missing config capability `debug config --pure`. Update OpenCode or set `brain env set opencode_cmd <command>` to a compatible command."
    );
}

#[test]
fn accepts_opencode_1_18_14_and_records_its_version() {
    let runner = FixtureRunner::new(
        "--agent <name>\n--prompt <text>\n--session <id>\n",
        "--format <format> json\n",
    );

    let report = inspect_with("opencode", &runner).expect("compatible OpenCode");

    assert_eq!(report.version(), Some("1.18.14"));
}

#[test]
fn accepts_a_future_version_when_its_features_are_compatible() {
    let runner = FixtureRunner::with_version(
        "9.4.0-next.2\n",
        "--agent <name>\n--prompt <text>\n--session <id>\n",
        "--format <format> json\n",
    );

    let report = inspect_with("opencode", &runner).expect("future compatible OpenCode");

    assert_eq!(report.version(), Some("9.4.0-next.2"));
}

#[test]
fn shell_runner_probes_a_configured_wrapper_with_ordinary_flags() {
    let temporary = tempfile::tempdir().expect("temporary command directory");
    let script = temporary.path().join("fake opencode");
    std::fs::write(
        &script,
        r#"#!/bin/sh
if [ "$1" = "--wrapper" ] && [ "$2" = "ordinary" ]; then
  shift 2
else
  exit 64
fi
case "$*" in
  --version) printf '1.18.14\n' ;;
  --help) printf '%s\n' '--agent <name>' '--prompt <text>' '--session <id>' ;;
  'session list --help') printf '%s\n' '--format <format> json' ;;
  'debug config --help') printf '%s\n' '--pure run without external plugins' ;;
  'debug config --pure'|'debug config') printf '{}\n' ;;
  *) exit 65 ;;
esac
"#,
    )
    .expect("write fake OpenCode");
    let mut permissions = std::fs::metadata(&script)
        .expect("fake metadata")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).expect("make fake executable");
    let quoted = script.display().to_string().replace('\'', "'\\''");
    let command = format!("'{quoted}' --wrapper ordinary");

    let runner = ShellProbeRunner::with_limits(Duration::from_secs(10), 32 * 1_024);
    let report = inspect_with(&command, &runner).expect("wrapped command");

    assert_eq!(report.version(), Some("1.18.14"));
}

#[test]
fn shell_runner_terminates_a_stalled_probe_at_its_deadline() {
    let runner = ShellProbeRunner::with_limits(Duration::from_millis(75), 1_024);
    let started = Instant::now();

    let error = runner
        .run("sleep 2;", &["--version"])
        .expect_err("stalled probe must time out");

    assert_eq!(error, ProbeRunError::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn process_group_termination_kills_the_wrapped_descendant() {
    let temporary = tempfile::tempdir().expect("temporary command directory");
    let script = temporary.path().join("stalled-opencode");
    let pid_path = temporary.path().join("descendant.pid");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$$\" > \"$BRAIN_PROBE_PID_FILE\"\nwhile :; do sleep 1; done\n",
    )
    .expect("write stalled command");
    let mut permissions = std::fs::metadata(&script)
        .expect("stalled metadata")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&script, permissions).expect("make stalled command executable");
    let command = format!(
        "BRAIN_PROBE_PID_FILE={} {}",
        shell_quote(&pid_path.display().to_string()),
        shell_quote(&script.display().to_string())
    );
    let mut child = Command::new("/bin/sh")
        .args(["-c", &format!("{command} --version")])
        .process_group(0)
        .spawn()
        .expect("spawn stalled process group");
    let ready_deadline = Instant::now() + Duration::from_secs(2);
    let pid = loop {
        if let Ok(contents) = std::fs::read_to_string(&pid_path)
            && let Ok(pid) = contents.trim().parse::<i32>()
        {
            break pid;
        }
        assert!(
            Instant::now() < ready_deadline,
            "stalled process group did not report a descendant PID"
        );
        thread::sleep(Duration::from_millis(5));
    };
    terminate_process_group(&mut child);
    child.wait().expect("reap process-group leader");

    let gone_deadline = Instant::now() + Duration::from_secs(1);
    while nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
        && Instant::now() < gone_deadline
    {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err(),
        "terminated descendant {pid} survived"
    );
}

#[test]
fn shell_runner_bounds_captured_stdout_and_stderr() {
    let runner = ShellProbeRunner::with_limits(Duration::from_secs(1), 128);

    let output = runner
        .run(
            "i=0; while [ $i -lt 1000 ]; do printf x; printf y >&2; i=$((i + 1)); done;",
            &["--version"],
        )
        .expect("bounded noisy probe");

    assert_eq!(output.stdout.len(), 128);
    assert_eq!(output.stderr.len(), 128);
}

#[test]
fn successful_probe_is_cached_once_per_exact_configured_command() {
    let runner = RecordingRunner::default();
    let cache = ProbeCache::default();

    inspect_cached_with("wrapper --profile one", &runner, &cache).expect("first probe");
    inspect_cached_with("wrapper --profile one", &runner, &cache).expect("cached probe");
    inspect_cached_with("wrapper --profile two", &runner, &cache).expect("distinct probe");

    let calls = runner.calls.lock().expect("recorded calls");
    assert_eq!(calls.len(), 12);
    assert!(
        calls[..6]
            .iter()
            .all(|(command, _)| command == "wrapper --profile one")
    );
    assert!(
        calls[6..]
            .iter()
            .all(|(command, _)| command == "wrapper --profile two")
    );
    drop(calls);
}

#[test]
fn concurrent_successful_checks_share_one_probe_flight() {
    let runner = RecordingRunner::default();
    let cache = ProbeCache::default();
    let start = Barrier::new(3);

    thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                start.wait();
                inspect_cached_with("opencode", &runner, &cache).expect("compatible probe");
            });
        }
        start.wait();
    });

    assert_eq!(runner.calls.lock().expect("recorded calls").len(), 6);
}
