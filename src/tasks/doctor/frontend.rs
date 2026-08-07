//! Registry-driven lifecycle integration health checks.

use std::path::Path;

/// Read-only lifecycle integration health for one registered frontend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontendHealth {
    kind: crate::agent::AgentKind,
    checks: Vec<HealthCheckResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HealthCheckResult {
    label: &'static str,
    ready: bool,
}

/// Read-only executable compatibility for one registered frontend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontendCompatibility {
    kind: crate::agent::AgentKind,
    version: Option<String>,
    error: Option<String>,
}

impl FrontendCompatibility {
    pub(super) fn from_result(
        kind: crate::agent::AgentKind,
        result: Result<Option<String>, crate::agent::AgentError>,
    ) -> Self {
        match result {
            Ok(version) => Self {
                kind,
                version,
                error: None,
            },
            Err(error) => Self {
                kind,
                version: None,
                error: Some(error.to_string()),
            },
        }
    }

    #[must_use]
    pub const fn kind(&self) -> crate::agent::AgentKind {
        self.kind
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.error.is_none()
    }

    #[must_use]
    pub fn detail(&self) -> &str {
        self.error
            .as_deref()
            .or(self.version.as_deref())
            .unwrap_or("compatible")
    }
}

impl FrontendHealth {
    /// Frontend this result describes.
    #[must_use]
    pub const fn kind(&self) -> crate::agent::AgentKind {
        self.kind
    }

    /// Whether every registered health check passed.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.checks.iter().all(|check| check.ready)
    }

    pub(super) fn check_ready(&self, label: &str) -> bool {
        self.checks
            .iter()
            .find(|check| check.label == label)
            .is_some_and(|check| check.ready)
    }
}

pub(super) fn inspect(workspace_root: &Path, home: &Path) -> Vec<FrontendHealth> {
    crate::agent::registrations()
        .iter()
        .map(|registration| FrontendHealth {
            kind: registration.kind(),
            checks: registration
                .health_checks()
                .iter()
                .map(|descriptor| HealthCheckResult {
                    label: descriptor.label(),
                    ready: check(*descriptor, workspace_root, home),
                })
                .collect(),
        })
        .collect()
}

pub(super) fn primary_session_check(
    workspace_root: &Path,
    home: &Path,
) -> Option<(std::path::PathBuf, &'static str, &'static str)> {
    crate::agent::primary_session_health_check().and_then(|descriptor| {
        match descriptor.expectation() {
            crate::agent::HealthCheckExpectation::Hook { event, suffix } => {
                Some((descriptor.path(workspace_root, home), event, suffix))
            }
            crate::agent::HealthCheckExpectation::FileContents(_) => None,
        }
    })
}

fn check(
    descriptor: crate::agent::HealthCheckDescriptor,
    workspace_root: &Path,
    home: &Path,
) -> bool {
    let path = descriptor.path(workspace_root, home);
    match descriptor.expectation() {
        crate::agent::HealthCheckExpectation::Hook { event, suffix } => {
            hook_command(&path, event, suffix).is_some()
        }
        crate::agent::HealthCheckExpectation::FileContents(expected) => {
            std::fs::read_to_string(path).is_ok_and(|actual| actual == expected)
        }
    }
}

/// Find Brain's SessionStart command in one frontend settings file.
pub(super) fn session_start_command(settings_path: &Path, suffix: &str) -> Option<String> {
    hook_command(settings_path, "SessionStart", suffix)
}

fn hook_command(settings_path: &Path, event: &str, suffix: &str) -> Option<String> {
    let raw = std::fs::read_to_string(settings_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let entries = value.get("hooks")?.get(event)?.as_array()?;
    entries
        .iter()
        .filter_map(|entry| entry.get("hooks").and_then(serde_json::Value::as_array))
        .flatten()
        .filter_map(|hook| hook.get("command").and_then(serde_json::Value::as_str))
        .find(|command| command.trim_end_matches(['"', '\'']).ends_with(suffix))
        .map(str::to_owned)
}
