//! Protected daily and weekly triage habit chains.

mod model;
mod purge;
mod reconcile;
mod transaction;

pub(crate) use model::protect_system_key;
pub use model::{
    DAILY_SYSTEM_KEY, ManagedTaskError, ManagedTriageHabit, WEEKLY_SYSTEM_KEY, can_complete,
    can_remove, can_revive, can_skip, is_managed_system_key,
};
pub use reconcile::apply_triage_habits_config;
