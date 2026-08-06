//! Protected daily and weekly triage habit chains.

mod complete_managed;
mod model;
mod purge;
mod reconcile;
mod transaction;

pub(crate) use complete_managed::run as complete_managed_triage_cli;
pub use complete_managed::{ManagedTriageCompletion, ManagedTriageKind, complete_managed_triage};
pub(crate) use model::protect_system_key;
pub use model::{
    DAILY_SYSTEM_KEY, ManagedTaskError, ManagedTriageHabit, WEEKLY_SYSTEM_KEY, can_complete,
    can_remove, can_revive, can_skip, is_managed_system_key,
};
pub use reconcile::apply_triage_habits_config;
pub(crate) use reconcile::apply_triage_habits_config_owned;
