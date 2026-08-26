use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use crate::agent::{
    AgentError,
    command_probe::{ProbeRunner, ShellProbeRunner},
};

const MINIMUM_VERSION: (u64, u64, u64) = (2, 1, 196);
const MINIMUM_VERSION_LABEL: &str = "2.1.196";
const UNAVAILABLE: &str = "Claude is unavailable: the configured command could not run. Install Claude Code 2.1.196 or later, or set `brain env set claude_cmd <command>` to a compatible command.";
const MALFORMED: &str = "Claude is incompatible: the configured command returned an unrecognized version. Update Claude Code to 2.1.196 or later, or set `brain env set claude_cmd <command>` to a compatible command.";

#[derive(Default)]
struct ProbeCache {
    successful: Mutex<HashMap<String, String>>,
}

static PROBE_CACHE: OnceLock<ProbeCache> = OnceLock::new();

pub(super) fn ensure_compatible(command: &str) -> Result<(), AgentError> {
    compatibility(command).map(|_| ())
}

pub(super) fn compatibility(command: &str) -> Result<Option<String>, AgentError> {
    inspect_cached_with(
        command,
        &ShellProbeRunner::default(),
        PROBE_CACHE.get_or_init(ProbeCache::default),
    )
    .map(Some)
}

fn inspect_cached_with(
    command: &str,
    runner: &dyn ProbeRunner,
    cache: &ProbeCache,
) -> Result<String, AgentError> {
    let mut successful = cache.successful.lock().expect("Claude probe cache lock");
    if let Some(version) = successful.get(command).cloned() {
        return Ok(version);
    }
    let version = inspect_with(command, runner)?;
    successful.insert(command.to_owned(), version.clone());
    drop(successful);
    Ok(version)
}

fn inspect_with(command: &str, runner: &dyn ProbeRunner) -> Result<String, AgentError> {
    let output = runner
        .run_isolated(command, &["--version"])
        .map_err(|_| AgentError::Frontend(UNAVAILABLE.to_owned()))?;
    if !output.success {
        return Err(AgentError::Frontend(UNAVAILABLE.to_owned()));
    }
    let (label, version) = parse_version(&output.combined_output())
        .ok_or_else(|| AgentError::Frontend(MALFORMED.to_owned()))?;
    if version < MINIMUM_VERSION {
        return Err(AgentError::Frontend(format!(
            "Claude is incompatible: version {label} does not provide the required `prompt_id` hook field. Update Claude Code to {MINIMUM_VERSION_LABEL} or later, or set `brain env set claude_cmd <command>` to a compatible command."
        )));
    }
    Ok(label)
}

fn parse_version(output: &str) -> Option<(String, (u64, u64, u64))> {
    output.split_whitespace().find_map(|token| {
        let candidate = token.trim_start_matches(['v', 'V']);
        let mut components = candidate.split('.');
        let major = components.next()?.parse::<u64>().ok()?;
        let minor = components.next()?.parse::<u64>().ok()?;
        let patch = components.next()?.parse::<u64>().ok()?;
        if components.next().is_some() {
            return None;
        }
        Some((candidate.to_owned(), (major, minor, patch)))
    })
}
