//! Consented execution against a fresh owned snapshot.
use super::model::{Options, Problem, RemovalPlan};
use super::operations::{CommitRecord, Operation, OperationOwner, Phase, Receipt};
use super::snapshot::{Snapshot, SnapshotContext};
use crate::tasks::store_lock::{Acquisition, TaskStoreOwner};
use crate::workspace::CommandContext;
use chrono::NaiveDate;

#[allow(clippy::needless_pass_by_value)] // Service contract owns its options.
pub(crate) fn preview(
    context: &CommandContext,
    needles: &[String],
    options: Options,
    today: NaiveDate,
) -> Result<RemovalPlan, Problem> {
    preview_with(context, needles, &options, today, Acquisition::Bounded)
}

pub(crate) fn preview_with(
    context: &CommandContext,
    needles: &[String],
    options: &Options,
    today: NaiveDate,
    acquisition: Acquisition,
) -> Result<RemovalPlan, Problem> {
    let owner = acquisition
        .inspect(&context.workspace)
        .map_err(|error| operational("snapshot_failed", &error))?;
    if crate::tasks::store_transaction::has_pending(&context.workspace, &owner)
        .map_err(|error| operational("snapshot_failed", &error))?
    {
        return Err(problem(
            "recovery_required",
            "Recover the pending task transaction before previewing removal",
        ));
    }
    let snapshot = snapshot(context, today, &owner)?;
    Ok(super::plan::plan(&snapshot, needles, options))
}

pub(crate) fn apply(
    context: &CommandContext,
    plan: &RemovalPlan,
    expected: &str,
    today: NaiveDate,
) -> Result<Receipt, Problem> {
    apply_with_hook(context, plan, expected, today, |_| Ok(()))
}

pub(super) fn apply_with_hook(
    context: &CommandContext,
    plan: &RemovalPlan,
    expected: &str,
    today: NaiveDate,
    hook: impl FnMut(crate::tasks::store_transaction::TransactionStep) -> std::io::Result<()>,
) -> Result<Receipt, Problem> {
    apply_queued(
        context,
        plan,
        expected,
        today,
        uuid::Uuid::new_v4().to_string(),
        Acquisition::Bounded,
        hook,
    )
}

pub(crate) fn apply_queued(
    context: &CommandContext,
    plan: &RemovalPlan,
    expected: &str,
    today: NaiveDate,
    operation_id: String,
    acquisition: Acquisition,
    hook: impl FnMut(crate::tasks::store_transaction::TransactionStep) -> std::io::Result<()>,
) -> Result<Receipt, Problem> {
    let ownership = OperationOwner::acquire(&context.workspace, &operation_id)
        .map_err(|error| operational("operation_unavailable", &error))?;
    let owner = acquisition
        .acquire(&context.workspace)
        .map_err(|error| operational("ownership_failed", &error))?;
    let snapshot = snapshot_for_plan(context, today, &owner, plan)?;
    validate(&snapshot, plan, expected)?;
    let canonical_inputs = serde_json::to_value(snapshot.canonical_for(
        &plan.options,
        &plan.targets,
        plan.maintenance.as_ref(),
    ))
    .map_err(|error| operational("snapshot_failed", &error.into()))?;
    let mut operation = Operation {
        schema_version: 1,
        operation_id,
        workspace_uuid: context.workspace.id().to_string(),
        actor_id: context.actor.user_id().to_string(),
        date: today.to_string(),
        fingerprint: expected.into(),
        canonical_inputs,
        plan: plan.clone(),
        phase: Phase::Prepared,
        external: plan.external.iter().map(Into::into).collect(),
        receipt: None,
    };
    ownership
        .save(&context.workspace, &operation)
        .map_err(|error| operational("operation_unavailable", &error))?;
    let result = (|| {
        if operation.external.is_empty() {
            commit(context, &mut operation, &owner, hook)?;
            drop(owner);
        } else {
            drop(owner);
            external_commit(context, &ownership, &mut operation, acquisition, hook)?;
        }
        finish(context, &ownership, &mut operation)
    })();
    result.map_err(|error| super::external::identified(&operation, error))
}

/// Network invocations hold operation ownership only. Reacquire task ownership
/// and compare the complete original consent after the last external action.
pub(super) fn external_commit(
    context: &CommandContext,
    ownership: &OperationOwner,
    operation: &mut Operation,
    acquisition: Acquisition,
    hook: impl FnMut(crate::tasks::store_transaction::TransactionStep) -> std::io::Result<()>,
) -> Result<(), Problem> {
    super::external::cancel(context, ownership, operation)?;
    let owner = acquisition.acquire(&context.workspace).map_err(|error| {
        super::external::identified(operation, operational("ownership_failed", &error))
    })?;
    revalidate(context, ownership, operation, &owner)?;
    commit(context, operation, &owner, hook)
}

pub(super) fn revalidate(
    context: &CommandContext,
    ownership: &OperationOwner,
    operation: &mut Operation,
    owner: &TaskStoreOwner,
) -> Result<(), Problem> {
    let result = snapshot_for_plan(
        context,
        chrono::Local::now().date_naive(),
        owner,
        &operation.plan,
    )
    .and_then(|current| validate(&current, &operation.plan, &operation.fingerprint));
    if let Err(error) = result {
        operation.phase = Phase::NeedsReconfirmation;
        super::external::save(context, ownership, operation)?;
        return Err(super::external::identified(operation, error));
    }
    Ok(())
}

pub(super) fn commit(
    context: &CommandContext,
    operation: &mut Operation,
    owner: &TaskStoreOwner,
    hook: impl FnMut(crate::tasks::store_transaction::TransactionStep) -> std::io::Result<()>,
) -> Result<(), Problem> {
    use crate::tasks::store_transaction::{FileChange, TargetPolicy, replace_group_recorded};
    let receipt = Receipt {
        operation_id: operation.operation_id.clone(),
        committed: true,
        removed: operation.plan.targets.clone(),
        updated: operation
            .plan
            .edits
            .iter()
            .map(|edit| edit.relative.to_string_lossy().into_owned())
            .collect(),
        external: operation.external.clone(),
        warnings: operation.plan.warnings.clone(),
    };
    let record = CommitRecord {
        schema_version: 1,
        workspace_uuid: operation.workspace_uuid.clone(),
        fingerprint: operation.fingerprint.clone(),
        receipt,
    };
    let changes: Vec<_> = operation
        .plan
        .edits
        .iter()
        .map(|edit| FileChange {
            path: context.workspace.root().join(&edit.relative),
            before: edit.before.clone(),
            after: edit.after.clone(),
        })
        .collect();
    let result = replace_group_recorded(
        &context.workspace,
        owner,
        TargetPolicy::TaskRemoval,
        &changes,
        Some(&record),
        hook,
    );
    // A cleanup error after commit must never be interpreted as rollback.
    let durable = super::operations::load_commit(&context.workspace, &operation.operation_id)
        .map_err(|error| operational("operation_unavailable", &error))?;
    if let Some(durable) = durable {
        operation.phase = Phase::LocalCommitted;
        operation.receipt = Some(durable.receipt);
        if let Err(error) = result {
            let _ = operational("transaction_cleanup_pending", &error);
        }
        return Ok(());
    }
    let error = result
        .err()
        .unwrap_or_else(|| anyhow::anyhow!("transaction did not retain a receipt"));
    let mut failure = operational("local_commit_pending", &error);
    failure.details = serde_json::json!({"operation_id": operation.operation_id});
    Err(failure)
}

pub(super) fn finish(
    context: &CommandContext,
    ownership: &OperationOwner,
    operation: &mut Operation,
) -> Result<Receipt, Problem> {
    let mut receipt = operation
        .receipt
        .as_ref()
        .expect("finish follows durable commit")
        .clone();
    let date = NaiveDate::parse_from_str(&operation.date, "%Y-%m-%d")
        .map_err(|error| operational("invalid_operation_date", &error.into()))?;
    let targets =
        crate::tasks::agenda::resolve_targets(&context.registry_store, &context.workspace, date);
    let changes: Vec<_> = operation
        .plan
        .targets
        .iter()
        .map(|target| crate::tasks::agenda::RemovalAgendaChange {
            removed_id: target.task_id.clone(),
            successor_id: operation.plan.successors.get(&target.task_id).cloned(),
        })
        .collect();
    if let crate::tasks::agenda::RemovalOutcome::Warning { stage } =
        crate::tasks::agenda::sync_removal_targets(&targets, &changes, date)
    {
        receipt.warnings.push(Problem::new(
            "agenda_refresh_failed",
            "Agenda refresh failed after local commit",
            serde_json::json!({"stage":stage.as_str()}),
        ));
    }
    // Downstream warnings belong to mutable progress, never the commit record.
    operation.receipt = Some(receipt.clone());
    operation.phase = Phase::Complete;
    // Receipt is already authoritative. Failure to save progress cannot undo it.
    if let Err(error) = ownership.save(&context.workspace, operation) {
        let _ = operational("operation_progress_pending", &error);
    }
    Ok(receipt)
}

pub(super) fn snapshot(
    context: &CommandContext,
    today: NaiveDate,
    owner: &TaskStoreOwner,
) -> Result<Snapshot, Problem> {
    let integration = super::integration::config::load(context)?;
    Snapshot::read_owned(
        &SnapshotContext {
            workspace: &context.workspace,
            actor: &context.actor,
            integration: &integration,
        },
        today,
        owner,
    )
}

pub(super) fn validate(
    snapshot: &Snapshot,
    plan: &RemovalPlan,
    expected: &str,
) -> Result<(), Problem> {
    let needles = plan
        .targets
        .iter()
        .map(|identity| identity.task_id.clone())
        .collect::<Vec<_>>();
    let current = super::plan::plan_with_maintenance(
        snapshot,
        &needles,
        &plan.options,
        plan.maintenance.clone(),
    );
    if plan.fingerprint != expected || !current.same_execution(plan) {
        return Err(problem(
            "stale_plan",
            "Task removal inputs changed; preview and confirm again",
        ));
    }
    if let Some(blocker) = current.blockers.first() {
        return Err(blocker.clone());
    }
    Ok(())
}

pub(super) fn problem(code: &str, message: &str) -> Problem {
    Problem::new(code, message, serde_json::json!({}))
}

pub(super) fn operational(code: &str, error: &anyhow::Error) -> Problem {
    // Preserve error kinds/context without serializing canonical data or credentials.
    crate::logging::log(format!("Task removal {code}: {}", diagnostic(error)));
    problem(code, "Task removal could not complete its local operation")
}

pub(super) fn diagnostic(error: &anyhow::Error) -> String {
    if let Some(error) = error.downcast_ref::<std::io::Error>() {
        return format!("io {:?} (OS code {:?})", error.kind(), error.raw_os_error());
    }
    if let Some(error) = error.downcast_ref::<serde_json::Error>() {
        return format!(
            "json {:?} at {}:{}",
            error.classify(),
            error.line(),
            error.column()
        );
    }
    if let Some(error) = error.downcast_ref::<rusqlite::Error>() {
        return format!("sqlite {:?}", error.sqlite_error_code());
    }
    // Arbitrary validation/context strings can contain task/config values.
    format!(
        "validation failure ({} error layers)",
        error.chain().count()
    )
}

fn snapshot_for_plan(
    context: &CommandContext,
    today: NaiveDate,
    owner: &TaskStoreOwner,
    plan: &RemovalPlan,
) -> Result<Snapshot, Problem> {
    let mut snapshot = snapshot(context, today, owner)?;
    super::maintenance::enrich(
        &mut snapshot,
        &context.workspace,
        &plan.targets,
        plan.maintenance.as_ref(),
    )?;
    Ok(snapshot)
}
