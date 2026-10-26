//! Read the native resolved prompt without loading external integrations.

use crate::agent::{AgentError, command_probe::ShellProbeRunner};
use serde_json::Value;
use std::{path::Path, time::Duration};

const OUTPUT_LIMIT: usize = 1024 * 1024;
const DIAGNOSTIC: &str = "cannot preserve OpenCode instructions; use a compatible OpenCode command with `debug config --pure` and literal arguments. Existing instructions were not replaced";

fn error() -> AgentError {
    AgentError::Frontend(DIAGNOSTIC.to_owned())
}

pub(super) fn validate_prefix(command: &str) -> Result<(), AgentError> {
    let words = crate::agent::direct_command::parse(command).ok_or_else(error)?;
    if words
        .iter()
        .take_while(|word| {
            word.contains('=')
                || Path::new(word)
                    .file_name()
                    .is_some_and(|name| name == "env")
        })
        .any(|word| word.starts_with("OPENCODE_CONFIG_CONTENT="))
    {
        return Err(AgentError::Frontend("opencode_cmd must not assign OPENCODE_CONFIG_CONTENT because it overrides Brain launch instructions; move that configuration to a native OpenCode config file or the inherited environment".to_owned()));
    }
    if !crate::agent::direct_command::accepts_appended_options(command) {
        return Err(error());
    }
    Ok(())
}

pub(super) fn resolve(
    command: &str,
    cwd: &Path,
    environment: &[(String, String)],
) -> Result<Option<String>, AgentError> {
    validate_prefix(command)?;
    let arguments = ["debug", "config", "--pure"];
    let bytes = arguments.iter().fold(command.len(), |total, argument| {
        total.saturating_add(crate::agent::frontend::shell_quote(argument).len() + 1)
    });
    if bytes > crate::agent::frontend::SHELL_COMMAND_ARGUMENT_BUDGET_BYTES {
        return Err(error());
    }
    let output = ShellProbeRunner::with_limits(Duration::from_secs(15), OUTPUT_LIMIT)
        .run_at_with_environment(command, &arguments, Some(cwd), Some(environment))
        .map_err(|_| error())?;
    if !output.success || output.stdout.len() >= OUTPUT_LIMIT {
        return Err(error());
    }
    let config: Value = serde_json::from_str(&output.stdout).map_err(|_| error())?;
    let config = config.as_object().ok_or_else(error)?;
    let Some(agent) = config.get("agent") else {
        return Ok(None);
    };
    let agent = agent.as_object().ok_or_else(error)?;
    let Some(brain) = agent.get("brain") else {
        return Ok(None);
    };
    let brain = brain.as_object().ok_or_else(error)?;
    brain
        .get("prompt")
        .map(|prompt| prompt.as_str().map(str::to_owned).ok_or_else(error))
        .transpose()
}
