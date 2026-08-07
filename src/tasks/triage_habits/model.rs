//! Stable identities and definitions for Brain-managed triage habits.

pub const DAILY_SYSTEM_KEY: &str = "brain.triage.daily";
pub const WEEKLY_SYSTEM_KEY: &str = "brain.triage.weekly";

/// The mutations a managed triage chain refuses while it is enabled.
///
/// Completion is deliberately absent: being managed means Brain owns the
/// chain's existence and cadence, not that the user may not tick an occurrence
/// off. Doing today's triage by hand and marking it done is exactly the
/// intended use; only removing, reviving, or skipping a managed row would
/// leave Brain's reconciliation with nothing to maintain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedTaskError {
    ManagedTaskCannotDelete,
    ManagedTaskCannotRevive,
    ManagedTaskCannotSkip,
}

impl std::fmt::Display for ManagedTaskError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ManagedTaskCannotDelete => {
                "managed triage habits cannot be deleted while enabled"
            }
            Self::ManagedTaskCannotRevive => {
                "managed triage habits cannot be revived manually while enabled"
            }
            Self::ManagedTaskCannotSkip => {
                "managed triage habits cannot be skipped manually while enabled"
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ManagedTaskError {}

#[must_use]
pub fn is_managed_system_key(value: &str) -> bool {
    matches!(value.trim(), DAILY_SYSTEM_KEY | WEEKLY_SYSTEM_KEY)
}

pub fn can_remove(
    task: &crate::tasks::task::Task,
    config: &crate::config::Config,
) -> Result<(), ManagedTaskError> {
    protect(task, config, ManagedTaskError::ManagedTaskCannotDelete)
}

pub fn can_revive(
    task: &crate::tasks::task::Task,
    config: &crate::config::Config,
) -> Result<(), ManagedTaskError> {
    protect(task, config, ManagedTaskError::ManagedTaskCannotRevive)
}

pub fn can_skip(
    task: &crate::tasks::task::Task,
    config: &crate::config::Config,
) -> Result<(), ManagedTaskError> {
    protect(task, config, ManagedTaskError::ManagedTaskCannotSkip)
}

fn protect(
    task: &crate::tasks::task::Task,
    config: &crate::config::Config,
    error: ManagedTaskError,
) -> Result<(), ManagedTaskError> {
    protect_system_key(&task.system_key, config.enable_triage_habits, error)
}

pub(crate) fn protect_system_key(
    system_key: &str,
    enabled: bool,
    error: ManagedTaskError,
) -> Result<(), ManagedTaskError> {
    if enabled && is_managed_system_key(system_key) {
        return Err(error);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedTriageHabit {
    pub system_key: &'static str,
    pub name: &'static str,
    pub interval: &'static str,
    pub unit: &'static str,
}

impl ManagedTriageHabit {
    pub const ALL: [Self; 2] = [
        Self {
            system_key: DAILY_SYSTEM_KEY,
            name: "Morning Triage",
            interval: "1",
            unit: "days",
        },
        Self {
            system_key: WEEKLY_SYSTEM_KEY,
            name: "Weekly in-basket processing",
            interval: "1",
            unit: "weeks",
        },
    ];
}
