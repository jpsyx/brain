//! Read-only OpenCode command compatibility probing.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use crate::agent::AgentError;

const UNAVAILABLE: &str = "OpenCode is unavailable: the configured command could not run. Install OpenCode or set `brain env set opencode_cmd <command>`.";

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

mod runner;

pub(super) use runner::read_only_output;
#[cfg(test)]
use runner::{ProbeOutput, ProbeRunError, terminate_process_group};
use runner::{ProbeRunner, ShellProbeRunner};

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
mod tests;
