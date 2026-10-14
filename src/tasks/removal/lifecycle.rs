//! Recovery and read-only observation of durable operations.
use super::{
    execute::{commit, external_commit, finish, operational, problem, revalidate},
    model::Problem,
    operations::{self, Operation, OperationOwner, Phase, Receipt},
};
use crate::{tasks::store_lock::TaskStoreOwner, workspace::CommandContext};

/// Atomic record reads only; status never takes task ownership or recovers.
pub(crate) fn operation_status(context: &CommandContext, id: &str) -> Result<Operation, Problem> {
    let operation = observe(context, id)?;
    if operation.receipt.is_none()
        && crate::tasks::store_transaction::committed_removal_pending(&context.workspace, id)
            .map_err(unavailable)?
    {
        return Err(problem(
            "recovery_required",
            "Removal committed; recover the pending receipt before inspecting its outcome",
        ));
    }
    // Recovery may publish a receipt and remove its journal between the first
    // receipt read and journal inspection. Re-read atomic evidence before returning.
    if operation.receipt.is_none() {
        return observe(context, id);
    }
    Ok(operation)
}

fn observe(context: &CommandContext, id: &str) -> Result<Operation, Problem> {
    let mut operation = operations::load(&context.workspace, id).map_err(unavailable)?;
    if let Some(mut record) =
        operations::load_commit(&context.workspace, id).map_err(unavailable)?
    {
        if record.fingerprint != operation.fingerprint {
            return Err(problem(
                "operation_unavailable",
                "Commit and operation fingerprints disagree",
            ));
        }
        if operation.phase != Phase::Complete {
            operation.phase = Phase::LocalCommitted;
        }
        if operation.phase == Phase::Complete {
            if let Some(completed) = &operation.receipt {
                for warning in &completed.warnings {
                    if !record.receipt.warnings.contains(warning) {
                        record.receipt.warnings.push(warning.clone());
                    }
                }
            }
        }
        operation.receipt = Some(record.receipt);
    } else if matches!(operation.phase, Phase::Complete | Phase::LocalCommitted)
        || operation
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.committed)
    {
        return Err(problem(
            "operation_unavailable",
            "Committed operation has no durable commit receipt",
        ));
    } else {
        // A progress-only failure receipt is not evidence of a local commit.
        operation.receipt = None;
    }
    Ok(operation)
}

pub(crate) fn resume(context: &CommandContext, id: &str) -> Result<Receipt, Problem> {
    let status = observe(context, id)?;
    actor(context, &status)?;
    if status.phase == Phase::Complete {
        return status
            .receipt
            .ok_or_else(|| problem("operation_unavailable", "Completed receipt is unavailable"));
    }
    let ownership = OperationOwner::acquire(&context.workspace, id).map_err(unavailable)?;
    let mut operation = observe(context, id)?;
    if operation.phase == Phase::Complete {
        return operation
            .receipt
            .ok_or_else(|| problem("operation_unavailable", "Completed receipt is unavailable"));
    }
    if operation.phase == Phase::Abandoned {
        return Err(problem(
            "operation_abandoned",
            "This removal operation was abandoned",
        ));
    }
    let owner = match TaskStoreOwner::acquire(&context.workspace) {
        Ok(owner) => owner,
        Err(error) => {
            let failure = operational("recovery_failed", &error);
            // Recovery can publish the receipt and then fail during cleanup.
            // Its durable outcome still wins over the operational error.
            if let Some(receipt) = observe(context, id)?.receipt {
                return Ok(receipt);
            }
            return Err(failure);
        }
    };
    // Recovery may have discovered a committed journal while progress said Prepared.
    operation = observe(context, id)?;
    if operation.receipt.is_some() {
        drop(owner);
        return finish(context, &ownership, &mut operation);
    }
    if operation.external.is_empty() {
        revalidate(context, &ownership, &mut operation, &owner)?;
        commit(context, &mut operation, &owner, |_| Ok(()))?;
        drop(owner);
    } else {
        drop(owner);
        super::external::reconcile(context, &ownership, &mut operation)?;
        let owner = TaskStoreOwner::acquire(&context.workspace).map_err(unavailable)?;
        revalidate(context, &ownership, &mut operation, &owner)?;
        drop(owner);
        external_commit(context, &ownership, &mut operation, |_| Ok(()))?;
    }
    finish(context, &ownership, &mut operation)
}

pub(crate) fn abandon(context: &CommandContext, id: &str) -> Result<Operation, Problem> {
    // Validate existence/scope before creating even a lock file.
    actor(context, &observe(context, id)?)?;
    let ownership = OperationOwner::try_acquire(&context.workspace, id)
        .map_err(unavailable)?
        .ok_or_else(|| {
            Problem::new(
                "operation_active",
                "A running removal cannot be abandoned",
                serde_json::json!({"operation_id": id}),
            )
        })?;
    if observe(context, id)?.receipt.is_some() {
        return Err(problem(
            "already_committed",
            "A committed removal cannot be abandoned",
        ));
    }
    let recovery = TaskStoreOwner::acquire(&context.workspace);
    let mut operation = observe(context, id)?;
    if operation.receipt.is_some() {
        return Err(problem(
            "already_committed",
            "A committed removal cannot be abandoned",
        ));
    }
    let owner = recovery.map_err(|error| operational("recovery_failed", &error))?;
    if operation
        .external
        .iter()
        .any(|action| action.state == operations::ExternalState::Unknown)
    {
        return Err(super::external::identified(
            &operation,
            problem(
                "external_pending",
                "An unknown external outcome must be reconciled before abandonment",
            ),
        ));
    }
    operation.phase = Phase::Abandoned;
    ownership
        .save(&context.workspace, &operation)
        .map_err(unavailable)?;
    drop(owner);
    Ok(operation)
}

#[allow(clippy::needless_pass_by_value)] // Result::map_err consumes the underlying error.
fn unavailable(error: anyhow::Error) -> Problem {
    operational("operation_unavailable", &error)
}

fn actor(context: &CommandContext, operation: &Operation) -> Result<(), Problem> {
    if operation.actor_id != context.actor.user_id().as_str() {
        return Err(super::external::identified(
            operation,
            problem(
                "actor_mismatch",
                "Only the original actor may resume or abandon this removal",
            ),
        ));
    }
    Ok(())
}
