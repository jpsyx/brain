use std::time::Duration;

use crate::agent::{AgentError, command_probe, frontend::shell_quote};

#[cfg(test)]
pub(super) use crate::agent::command_probe::terminate_process_group;
pub(super) use crate::agent::command_probe::{ProbeOutput, ProbeRunError};

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
    shared: command_probe::ShellProbeRunner,
    output_limit: usize,
}

impl ShellProbeRunner {
    #[cfg(test)]
    pub(super) const fn with_limits(timeout: Duration, output_limit: usize) -> Self {
        Self {
            shared: command_probe::ShellProbeRunner::with_limits(timeout, output_limit),
            output_limit,
        }
    }

    fn run_at(
        &self,
        command: &str,
        arguments: &[&str],
        cwd: Option<&std::path::Path>,
    ) -> Result<ProbeOutput, ProbeRunError> {
        self.shared.run_at(command, arguments, cwd)
    }
}

impl Default for ShellProbeRunner {
    fn default() -> Self {
        Self {
            shared: command_probe::ShellProbeRunner::default(),
            output_limit: 32 * 1_024,
        }
    }
}

impl ProbeRunner for ShellProbeRunner {
    fn run(&self, command: &str, arguments: &[&str]) -> Result<ProbeOutput, ProbeRunError> {
        command_probe::ProbeRunner::run(&self.shared, command, arguments)
    }

    fn run_isolated(
        &self,
        command: &str,
        arguments: &[&str],
    ) -> Result<ProbeOutput, ProbeRunError> {
        command_probe::ProbeRunner::run_isolated(&self.shared, command, arguments)
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
            command_probe::ShellProbeRunner::with_limits(Duration::from_secs(15), self.output_limit)
                .run_at(&isolated, arguments, Some(&directory))
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
