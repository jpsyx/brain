//! Compatibility facade over the consented native removal service.
use crate::workspace::CommandContext;
use anyhow::Result;
use chrono::NaiveDate;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoveResult {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) was_habit: bool,
}

/// Test-only forwarding shim retains the legacy single-target regression checks.
pub(crate) fn remove(
    context: &CommandContext,
    raw_id: &str,
    allow_habit: bool,
    today: NaiveDate,
) -> Result<RemoveResult> {
    use crate::tasks::removal::{apply, model::Options, preview};
    let plan = preview(
        context,
        &[raw_id.into()],
        Options {
            habit: allow_habit,
            ..Options::default()
        },
        today,
    )?;
    if let Some(blocker) = plan.blockers.first() {
        return Err(blocker.clone().into());
    }
    let receipt = apply(context, &plan, &plan.fingerprint, today)?;
    let identity = &receipt.removed[0];
    Ok(RemoveResult {
        task_id: identity.task_id.clone(),
        task_name: identity.task_name.clone(),
        was_habit: identity.csv == crate::tasks::identity::CsvKind::Habits,
    })
}
