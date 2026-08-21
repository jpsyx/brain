use std::{
    io::Read,
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::agent::{AgentError, frontend::shell_quote};

#[derive(Debug)]
pub(super) struct ProbeOutput {
    pub(super) success: bool,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

impl ProbeOutput {
    #[cfg(test)]
    pub(super) fn success(stdout: &str) -> Self {
        Self {
            success: true,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    pub(super) fn combined_output(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProbeRunError {
    Start,
    Wait,
    Capture,
    TimedOut,
}

pub(super) trait ProbeRunner {
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

pub(super) struct ShellProbeRunner {
    timeout: Duration,
    output_limit: usize,
}

impl ShellProbeRunner {
    pub(super) const fn with_limits(timeout: Duration, output_limit: usize) -> Self {
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

pub(super) fn terminate_process_group(child: &mut std::process::Child) {
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
                &crate::agent::opencode::config::compatibility_probe(&directory)
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

pub(in crate::agent::opencode) fn read_only_output(
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
