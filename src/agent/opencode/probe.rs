//! Read-only OpenCode command compatibility probing.

use std::{
    collections::HashMap,
    io::Read,
    process::Command,
    sync::{Mutex, OnceLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use crate::agent::{AgentError, frontend::shell_quote};

const UNAVAILABLE: &str = "OpenCode is unavailable: the configured command could not run. Install OpenCode or set `brain env set opencode_cmd <command>`.";

#[derive(Debug)]
struct ProbeOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

impl ProbeOutput {
    #[cfg(test)]
    fn success(stdout: &str) -> Self {
        Self {
            success: true,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    fn combined_output(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeRunError {
    Start,
    Wait,
    Capture,
    TimedOut,
}

trait ProbeRunner {
    fn run(&self, command: &str, arguments: &[&str]) -> Result<ProbeOutput, ProbeRunError>;

    fn run_isolated(
        &self,
        command: &str,
        arguments: &[&str],
    ) -> Result<ProbeOutput, ProbeRunError> {
        self.run(command, arguments)
    }

    fn run_config(&self, command: &str, load_plugin: bool) -> Result<ProbeOutput, ProbeRunError> {
        let arguments = if load_plugin {
            &["debug", "config"][..]
        } else {
            &["debug", "config", "--pure"][..]
        };
        self.run_isolated(command, arguments)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompatibilityReport {
    version: Option<String>,
}

impl CompatibilityReport {
    #[must_use]
    pub(crate) fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

struct ShellProbeRunner {
    timeout: Duration,
    output_limit: usize,
}

impl ShellProbeRunner {
    const fn with_limits(timeout: Duration, output_limit: usize) -> Self {
        Self {
            timeout,
            output_limit,
        }
    }

    fn run_at(
        &self,
        command: &str,
        arguments: &[&str],
        cwd: Option<&std::path::Path>,
    ) -> Result<ProbeOutput, ProbeRunError> {
        use std::os::unix::process::CommandExt as _;

        let arguments = arguments
            .iter()
            .map(|argument| shell_quote(argument))
            .collect::<Vec<_>>()
            .join(" ");
        let probe = format!("{command} {arguments}");
        let mut shell = Command::new("/bin/sh");
        shell
            .args(["-c", &probe])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .process_group(0);
        if let Some(cwd) = cwd {
            shell.current_dir(cwd);
        }
        let mut child = shell.spawn().map_err(|_| ProbeRunError::Start)?;
        let stdout = child.stdout.take().ok_or(ProbeRunError::Capture)?;
        let stderr = child.stderr.take().ok_or(ProbeRunError::Capture)?;
        let stdout_rx = capture_bounded(stdout, self.output_limit);
        let stderr_rx = capture_bounded(stderr, self.output_limit);
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait().map_err(|_| ProbeRunError::Wait)? {
                Some(status) => break status,
                None if Instant::now() >= deadline => {
                    terminate_process_group(&mut child);
                    let _ = child.wait();
                    return Err(ProbeRunError::TimedOut);
                }
                None => thread::sleep(Duration::from_millis(5)),
            }
        };
        let stdout = receive_capture(&stdout_rx, deadline)?;
        let stderr = receive_capture(&stderr_rx, deadline)?;
        Ok(ProbeOutput {
            success: status.success(),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }
}

fn terminate_process_group(child: &mut std::process::Child) {
    let Ok(pid) = i32::try_from(child.id()) else {
        let _ = child.kill();
        return;
    };
    if nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGKILL,
    )
    .is_err()
    {
        let _ = child.kill();
    }
}

impl Default for ShellProbeRunner {
    fn default() -> Self {
        Self::with_limits(Duration::from_secs(2), 32 * 1_024)
    }
}

impl ProbeRunner for ShellProbeRunner {
    fn run(&self, command: &str, arguments: &[&str]) -> Result<ProbeOutput, ProbeRunError> {
        self.run_at(command, arguments, None)
    }

    fn run_isolated(
        &self,
        command: &str,
        arguments: &[&str],
    ) -> Result<ProbeOutput, ProbeRunError> {
        let directory = std::env::temp_dir().join(format!(
            "brain-opencode-probe-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&directory).map_err(|_| ProbeRunError::Start)?;
        let path = shell_quote(&directory.display().to_string());
        let isolated = format!(
            "HOME={path} XDG_CONFIG_HOME={path} XDG_CACHE_HOME={path} XDG_DATA_HOME={path} XDG_STATE_HOME={path} {command}"
        );
        let result = self.run_at(&isolated, arguments, None);
        let _ = std::fs::remove_dir_all(directory);
        result
    }

    fn run_config(&self, command: &str, load_plugin: bool) -> Result<ProbeOutput, ProbeRunError> {
        let directory = std::env::temp_dir().join(format!(
            "brain-opencode-config-probe-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&directory).map_err(|_| ProbeRunError::Start)?;
        let result = (|| {
            if load_plugin {
                let plugin = directory.join(".opencode/plugins/brain.js");
                std::fs::create_dir_all(plugin.parent().ok_or(ProbeRunError::Start)?)
                    .map_err(|_| ProbeRunError::Start)?;
                std::fs::write(
                    plugin,
                    include_str!(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/scripts/opencode_brain_plugin.js"
                    )),
                )
                .map_err(|_| ProbeRunError::Start)?;
            }
            let path = shell_quote(&directory.display().to_string());
            let config = shell_quote(
                &super::config::compatibility_probe(&directory)
                    .map_err(|_| ProbeRunError::Start)?,
            );
            let isolated = format!(
                "HOME={path} XDG_CONFIG_HOME={path} XDG_CACHE_HOME={path} XDG_DATA_HOME={path} XDG_STATE_HOME={path} BRAIN_ROOT={path} BRAIN_AGENT_KIND=opencode OPENCODE_CONFIG_CONTENT={config} {command}"
            );
            let arguments = if load_plugin {
                &["debug", "config"][..]
            } else {
                &["debug", "config", "--pure"][..]
            };
            Self::with_limits(Duration::from_secs(15), self.output_limit).run_at(
                &isolated,
                arguments,
                Some(&directory),
            )
        })();
        let _ = std::fs::remove_dir_all(directory);
        result
    }
}

pub(super) fn read_only_output(
    command: &str,
    arguments: &[&str],
    cwd: &std::path::Path,
    operation: &str,
) -> Result<String, AgentError> {
    let output = ShellProbeRunner::default()
        .run_at(command, arguments, Some(cwd))
        .map_err(|error| {
            AgentError::Frontend(format!(
                "OpenCode {operation} could not complete ({})",
                run_error_label(error)
            ))
        })?;
    if !output.success {
        return Err(AgentError::Frontend(format!(
            "OpenCode {operation} failed; run `opencode session list --format json` in the selected workspace"
        )));
    }
    Ok(output.stdout)
}

const fn run_error_label(error: ProbeRunError) -> &'static str {
    match error {
        ProbeRunError::Start => "command could not start",
        ProbeRunError::Wait => "command status was unavailable",
        ProbeRunError::Capture => "command output could not be captured",
        ProbeRunError::TimedOut => "command timed out",
    }
}

fn capture_bounded(
    mut reader: impl Read + Send + 'static,
    limit: usize,
) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut captured = Vec::with_capacity(limit.min(8 * 1_024));
        let mut buffer = [0_u8; 8 * 1_024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let remaining = limit.saturating_sub(captured.len());
                    captured.extend_from_slice(&buffer[..read.min(remaining)]);
                }
            }
        }
        let _ = sender.send(captured);
    });
    receiver
}

fn receive_capture(
    receiver: &mpsc::Receiver<Vec<u8>>,
    deadline: Instant,
) -> Result<Vec<u8>, ProbeRunError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    receiver
        .recv_timeout(remaining)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => ProbeRunError::TimedOut,
            mpsc::RecvTimeoutError::Disconnected => ProbeRunError::Capture,
        })
}

#[derive(Default)]
struct ProbeCache {
    successful: Mutex<HashMap<String, CompatibilityReport>>,
}

static PROBE_CACHE: OnceLock<ProbeCache> = OnceLock::new();

pub(super) fn ensure_compatible(command: &str) -> Result<(), AgentError> {
    compatibility(command).map(|_| ())
}

pub(super) fn compatibility(command: &str) -> Result<CompatibilityReport, AgentError> {
    inspect_cached_with(
        command,
        &ShellProbeRunner::default(),
        PROBE_CACHE.get_or_init(ProbeCache::default),
    )
}

fn inspect_cached_with(
    command: &str,
    runner: &dyn ProbeRunner,
    cache: &ProbeCache,
) -> Result<CompatibilityReport, AgentError> {
    let mut successful = cache.successful.lock().expect("OpenCode probe cache lock");
    if let Some(report) = successful.get(command).cloned() {
        return Ok(report);
    }
    let report = inspect_with(command, runner)?;
    successful.insert(command.to_owned(), report.clone());
    drop(successful);
    Ok(report)
}

fn inspect_with(
    command: &str,
    runner: &dyn ProbeRunner,
) -> Result<CompatibilityReport, AgentError> {
    let version = runner
        .run_isolated(command, &["--version"])
        .map_err(|_| AgentError::Frontend(UNAVAILABLE.to_owned()))?;
    if !version.success {
        return Err(AgentError::Frontend(UNAVAILABLE.to_owned()));
    }
    let help = runner
        .run_isolated(command, &["--help"])
        .map_err(|_| AgentError::Frontend(UNAVAILABLE.to_owned()))?;
    if !help.success {
        return Err(AgentError::Frontend(UNAVAILABLE.to_owned()));
    }
    let session_help = runner
        .run_isolated(command, &["session", "list", "--help"])
        .map_err(|_| AgentError::Frontend(UNAVAILABLE.to_owned()))?;
    if !session_help.success {
        return Err(AgentError::Frontend(UNAVAILABLE.to_owned()));
    }
    let help_output = help.combined_output();
    for option in ["--agent", "--prompt", "--session"] {
        if !has_option(&help_output, option) {
            return Err(incompatible("TUI", option));
        }
    }
    let session_output = session_help.combined_output();
    if !has_option(&session_output, "--format")
        || !session_output.split_whitespace().any(|token| {
            token
                .trim_matches(|character: char| !character.is_alphanumeric())
                .eq_ignore_ascii_case("json")
        })
    {
        return Err(incompatible("session-list", "--format json"));
    }
    let config_help = runner
        .run_isolated(command, &["debug", "config", "--help"])
        .map_err(|_| incompatible("config", "debug config --pure"))?;
    if !config_help.success || !has_option(&config_help.combined_output(), "--pure") {
        return Err(incompatible("config", "debug config --pure"));
    }
    for (load_plugin, capability) in [
        (false, "generated capability schema"),
        (true, "Brain lifecycle plugin"),
    ] {
        let resolved = runner
            .run_config(command, load_plugin)
            .map_err(|_| incompatible("config", capability))?;
        if !resolved.success
            || serde_json::from_str::<serde_json::Value>(&resolved.stdout)
                .ok()
                .and_then(|value| value.as_object().cloned())
                .is_none()
        {
            return Err(incompatible("config", capability));
        }
    }
    Ok(CompatibilityReport {
        version: parse_version(&version.combined_output()),
    })
}

fn has_option(output: &str, option: &str) -> bool {
    output.split_whitespace().any(|token| {
        token.trim_matches(|character: char| character == ',' || character == ':') == option
    })
}

fn incompatible(surface: &str, capability: &str) -> AgentError {
    AgentError::Frontend(format!(
        "OpenCode is incompatible: missing {surface} capability `{capability}`. Update OpenCode or set `brain env set opencode_cmd <command>` to a compatible command."
    ))
}

fn parse_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let candidate = token.trim_start_matches(['v', 'V']);
        (candidate.len() <= 64
            && candidate.starts_with(|character: char| character.is_ascii_digit())
            && candidate.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+')
            }))
        .then(|| candidate.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use std::{
        os::unix::{fs::PermissionsExt, process::CommandExt as _},
        sync::{Barrier, Mutex},
        time::{Duration, Instant},
    };

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
}
