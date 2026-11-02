//! Speculative consent is checked against owned data before any side effect.
use super::{
    execute::{operational, problem},
    model::{Options, Problem, RemovalPlan},
    snapshot::{Snapshot, SnapshotContext},
};
use crate::{tasks::complete::field, workspace::CommandContext};
use chrono::NaiveDate;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn preview(
    context: &CommandContext,
    targets: &[String],
    options: &Options,
    today: NaiveDate,
) -> Result<RemovalPlan, Problem> {
    let integration = super::integration::config::load(context)?;
    let snapshot = Snapshot::read_observed(
        &SnapshotContext {
            workspace: &context.workspace,
            actor: &context.actor,
            integration: &integration,
        },
        today,
    )?;
    let mut plan = super::plan::plan(&snapshot, targets, options);
    plan.pending_consent = Some(consent(&snapshot, &plan)?);
    Ok(plan)
}

pub(super) fn rebase(snapshot: &Snapshot, original: &RemovalPlan) -> Result<RemovalPlan, Problem> {
    let Some(expected) = &original.pending_consent else {
        return Ok(original.clone());
    };
    let targets = original
        .targets
        .iter()
        .map(|target| target.task_id.clone())
        .collect::<Vec<_>>();
    let current = super::plan::plan(snapshot, &targets, &original.options);
    if !current.blockers.is_empty() || &consent(snapshot, &current)? != expected {
        return Err(problem(
            "stale_plan",
            "Deletion canceled: task details or deletion consequences changed while waiting. Review and retry",
        ));
    }
    Ok(current)
}

fn consent(snapshot: &Snapshot, plan: &RemovalPlan) -> Result<String, Problem> {
    let mut effects = Vec::new();
    for edit in &plan.edits {
        let effect = if matches!(
            edit.relative.to_str(),
            Some("tasks/tasks.csv" | "tasks/habits.csv")
        ) {
            let rows = |bytes: &[u8]| -> Result<BTreeMap<_, _>, Problem> {
                let parsed = super::snapshot::parse_raw_csv(bytes)
                    .map_err(|error| operational("snapshot_failed", &error))?;
                Ok(parsed
                    .rows
                    .into_iter()
                    .map(|row| (field(&row, "task_id"), row))
                    .collect())
            };
            let before = rows(edit.before.as_deref().unwrap_or_default())?;
            let after = rows(&edit.after)?;
            let ids: BTreeSet<_> = before.keys().chain(after.keys()).collect();
            let changed: BTreeMap<_, _> = ids
                .into_iter()
                .filter(|id| before.get(*id) != after.get(*id))
                .map(|id| (id, (before.get(id), after.get(id))))
                .collect();
            serde_json::json!({"path": edit.relative, "rows": changed})
        } else {
            serde_json::to_value(edit)
                .map_err(|error| operational("snapshot_failed", &error.into()))?
        };
        effects.push(effect);
    }
    super::fingerprint::digest(&serde_json::json!({
        "workspace": snapshot.workspace_uuid,
        "actor": snapshot.actor_id,
        "date": snapshot.execution_date,
        "integration": snapshot.integration,
        "managed_habits": snapshot.managed_habits_enabled,
        "targets": plan.targets,
        "options": plan.options,
        "effects": effects,
        "external": plan.external,
        "successors": plan.successors,
        "labels": plan.presentation_labels,
        "blockers": plan.blockers,
    }))
    .map_err(|error| operational("snapshot_failed", &error))
}
