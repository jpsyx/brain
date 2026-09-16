//! Read-only pi command compatibility and configuration probing.
//!
//! Three questions, in the order a user hits them: can the configured command
//! run at all, is it new enough to expose everything Brain appends to it, and
//! is this machine's pi configured with a model it can actually run a turn
//! with. The first two use a disposable configuration root; the third is a
//! question *about* the user's configuration, so it reads the real one, offline
//! and without writing.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use crate::agent::{
    AgentError,
    command_probe::{ProbeOutput, ProbeRunError, ProbeRunner as _, ShellProbeRunner},
};

#[cfg(test)]
mod tests;

/// The lowest pi release carrying every surface Brain depends on: `--session-id`
/// (0.76.0), the `agent_settled` extension event and the `--session-id` create
/// warning (0.80.4), and the auth-readiness surfaces (0.84.1).
const MINIMUM_VERSION: (u64, u64, u64) = (0, 84, 1);
const MINIMUM_VERSION_LABEL: &str = "0.84.1";

/// Flags Brain appends to every pi launch.
const REQUIRED_FLAGS: [&str; 6] = [
    "--session-id",
    "--append-system-prompt",
    "--extension",
    "--skill",
    "--no-skills",
    "--no-approve",
];

const UNAVAILABLE: &str = "pi is unavailable: the configured command could not run. Install pi 0.84.1 or later, or set `brain env set pi_cmd <command>` to a compatible command.";
const MALFORMED: &str = "pi is incompatible: the configured command returned an unrecognized version. Update pi to 0.84.1 or later, or set `brain env set pi_cmd <command>` to a compatible command.";
const UNREADABLE_CATALOG: &str = "pi is not configured: `pi --list-models` could not be read on this machine. Run `pi --list-models` yourself, or set `brain env set pi_cmd <command>` to a working command.";
const NO_MODELS: &str = "pi is not configured: no model's provider credentials resolve on this machine, so the brain panel could not run a turn. Run `pi` and use `/login`, or export the provider's API key, then check `pi --list-models`.";

/// Timeout for the model catalog, which reads the user's providers rather than
/// printing a fixed string.
const CATALOG_TIMEOUT: Duration = Duration::from_secs(15);
const CATALOG_OUTPUT_LIMIT: usize = 64 * 1_024;

#[derive(Default)]
struct ProbeCache {
    successful: Mutex<HashMap<String, String>>,
}

static PROBE_CACHE: OnceLock<ProbeCache> = OnceLock::new();

pub(super) fn ensure_compatible(command: &str) -> Result<(), AgentError> {
    compatibility(command).map(|_| ())
}

pub(super) fn compatibility(command: &str) -> Result<Option<String>, AgentError> {
    inspect_cached(command, PROBE_CACHE.get_or_init(ProbeCache::default)).map(Some)
}

fn inspect_cached(command: &str, cache: &ProbeCache) -> Result<String, AgentError> {
    let mut successful = cache.successful.lock().expect("pi probe cache lock");
    if let Some(version) = successful.get(command).cloned() {
        return Ok(version);
    }
    let version = inspect(command)?;
    successful.insert(command.to_owned(), version.clone());
    drop(successful);
    Ok(version)
}

fn inspect(command: &str) -> Result<String, AgentError> {
    let runner = ShellProbeRunner::default();
    let version = runner
        .run_isolated(command, &["--version"])
        .map_err(|_| AgentError::Frontend(UNAVAILABLE.to_owned()))?;
    if !version.success {
        return Err(AgentError::Frontend(UNAVAILABLE.to_owned()));
    }
    let (label, parsed) =
        parse_version(&version.combined_output()).ok_or_else(|| incompatible_version(None))?;
    if parsed < MINIMUM_VERSION {
        return Err(incompatible_version(Some(&label)));
    }
    let help = runner
        .run_isolated(command, &["--help"])
        .map_err(|_| AgentError::Frontend(UNAVAILABLE.to_owned()))?;
    if !help.success {
        return Err(AgentError::Frontend(UNAVAILABLE.to_owned()));
    }
    let help_output = help.combined_output();
    for flag in REQUIRED_FLAGS {
        if !advertises(&help_output, flag) {
            return Err(missing_flag(flag));
        }
    }
    let catalog = run_catalog(command).map_err(|_| AgentError::Frontend(UNAVAILABLE.to_owned()))?;
    if !catalog.success {
        return Err(AgentError::Frontend(UNREADABLE_CATALOG.to_owned()));
    }
    if !lists_a_model(&catalog.stdout) {
        return Err(AgentError::Frontend(NO_MODELS.to_owned()));
    }
    Ok(label)
}

/// pi's model catalog, read from the machine's real configuration.
///
/// `PI_OFFLINE=1` keeps the read from touching the network, so the probe stays
/// fast, works on a plane, and never updates a catalog as a side effect.
fn run_catalog(command: &str) -> Result<ProbeOutput, ProbeRunError> {
    ShellProbeRunner::with_limits(CATALOG_TIMEOUT, CATALOG_OUTPUT_LIMIT).run_at(
        &format!("PI_OFFLINE=1 {command}"),
        &["--list-models"],
        None,
    )
}

/// Whether `pi --list-models` printed its table header and at least one model.
///
/// An unconfigured pi prints `No models available.` instead, and a pi with no
/// resolvable provider credentials prints exactly that too, so the presence of
/// a data row is the honest signal.
fn lists_a_model(stdout: &str) -> bool {
    let mut rows = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    rows.next()
        .is_some_and(|header| header.starts_with("provider"))
        && rows.next().is_some()
}

/// Whether a help listing advertises `flag` as its own option token.
fn advertises(output: &str, flag: &str) -> bool {
    output.split_whitespace().any(|token| {
        token.trim_matches(|character: char| matches!(character, ',' | ':' | '.')) == flag
    })
}

fn incompatible_version(found: Option<&str>) -> AgentError {
    found.map_or_else(
        || AgentError::Frontend(MALFORMED.to_owned()),
        |label| {
            AgentError::Frontend(format!(
                "pi is incompatible: version {label} is older than the {MINIMUM_VERSION_LABEL} Brain requires for caller-chosen session ids and settled-turn completion. Update pi, or set `brain env set pi_cmd <command>` to a compatible command."
            ))
        },
    )
}

fn missing_flag(flag: &str) -> AgentError {
    AgentError::Frontend(format!(
        "pi is incompatible: the configured command does not advertise `{flag}`. Update pi to {MINIMUM_VERSION_LABEL} or later, or set `brain env set pi_cmd <command>` to a compatible command."
    ))
}

/// pi prints a bare version (`0.85.1`), so the whole line must be one version
/// token: anything else is some other program answering `--version`.
fn parse_version(output: &str) -> Option<(String, (u64, u64, u64))> {
    let candidate = output.trim();
    let candidate = candidate.strip_prefix('v').unwrap_or(candidate);
    let numeric = candidate
        .split_once(['-', '+'])
        .map_or(candidate, |(head, _)| head);
    let mut components = numeric.split('.');
    let major = components.next()?.parse::<u64>().ok()?;
    let minor = components.next()?.parse::<u64>().ok()?;
    let patch = components.next()?.parse::<u64>().ok()?;
    if components.next().is_some() {
        return None;
    }
    Some((candidate.to_owned(), (major, minor, patch)))
}
