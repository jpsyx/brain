use super::{DuplicateReplacement, MaintenanceIntent};
use crate::{
    tasks::{
        backlog::{dedupe, minus_six_months, purge},
        removal::{
            execute,
            model::{Problem, RemovalPlan},
        },
        store_lock::TaskStoreOwner,
    },
    workspace::CommandContext,
};
use chrono::NaiveDate;
#[derive(Clone, Copy)]
pub(crate) enum MaintenanceKind {
    Purge,
    Dedupe,
}
pub(crate) struct MaintenancePreview {
    pub cutoff: Option<String>,
    pub purged: Vec<purge::PurgedTask>,
    pub superseded: Vec<dedupe::Superseded>,
    pub skipped: Vec<dedupe::Skipped>,
    pub plan: Option<RemovalPlan>,
}
pub(crate) fn preview_maintenance(
    context: &CommandContext,
    kind: MaintenanceKind,
    today: NaiveDate,
) -> Result<MaintenancePreview, Problem> {
    let owner = TaskStoreOwner::acquire_inspection(&context.workspace)
        .map_err(|error| execute::operational("snapshot_failed", &error))?;
    if crate::tasks::store_transaction::has_pending(&context.workspace, &owner)
        .map_err(|error| execute::operational("snapshot_failed", &error))?
    {
        return Err(execute::problem(
            "recovery_required",
            "Recover the pending task transaction before previewing removal",
        ));
    }
    let mut snapshot = execute::snapshot(context, today, &owner)?;
    let mut result = MaintenancePreview {
        cutoff: None,
        purged: vec![],
        superseded: vec![],
        skipped: vec![],
        plan: None,
    };
    let (targets, intent) = match kind {
        MaintenanceKind::Purge => {
            let cutoff = minus_six_months(today).ok_or_else(|| {
                execute::problem("invalid_maintenance", "Cannot compute six-month cutoff")
            })?;
            result.cutoff = Some(cutoff.to_string());
            result.purged = purge::expired(&snapshot.tasks.parsed.rows, cutoff);
            (
                result
                    .purged
                    .iter()
                    .map(|task| task.task_id.clone())
                    .collect::<Vec<_>>(),
                MaintenanceIntent::BacklogPurge {
                    cutoff: cutoff.to_string(),
                },
            )
        }
        MaintenanceKind::Dedupe => {
            (result.superseded, result.skipped) = dedupe::candidates(&snapshot.tasks.parsed.rows);
            let identity = |id: &str| {
                snapshot
                    .tasks
                    .identities
                    .iter()
                    .find(|identity| identity.task_id == id)
                    .cloned()
                    .ok_or_else(|| {
                        execute::problem(
                            "invalid_maintenance",
                            "Maintenance candidate identity missing",
                        )
                    })
            };
            let replacements = result
                .superseded
                .iter()
                .map(|task| {
                    Ok(DuplicateReplacement {
                        duplicate: identity(&task.task_id)?,
                        survivor: identity(&task.survivor_id)?,
                    })
                })
                .collect::<Result<Vec<_>, Problem>>()?;
            (
                result
                    .superseded
                    .iter()
                    .map(|task| task.task_id.clone())
                    .collect(),
                MaintenanceIntent::BacklogDedupe { replacements },
            )
        }
    };
    if targets.is_empty() {
        return Ok(result);
    }
    let selected = snapshot.selected_identities(&targets, &super::options())?;
    super::enrich(&mut snapshot, &context.workspace, &selected, Some(&intent))?;
    result.plan = Some(crate::tasks::removal::plan::plan_with_maintenance(
        &snapshot,
        &targets,
        &super::options(),
        Some(intent),
    ));
    Ok(result)
}
