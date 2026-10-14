//! Durable external intent and acknowledgement under operation ownership only.
use super::{
    execute::{operational, problem},
    integration::{config, process, protocol::Request},
    model::{AdapterSnapshot, IntegrationSnapshot, Problem},
    operations::{ExternalState, Operation, OperationOwner, Phase},
};
use crate::workspace::CommandContext;

/// Reconcile only saved unknown actions; changed local rows do not prevent
/// learning history, but no different adapter may be substituted under consent.
pub(super) fn reconcile(
    context: &CommandContext,
    ownership: &OperationOwner,
    operation: &mut Operation,
) -> Result<(), Problem> {
    if !operation
        .external
        .iter()
        .any(|action| action.state == ExternalState::Unknown)
    {
        return Ok(());
    }
    let adapter = adapter(context, ownership, operation)?;
    for index in 0..operation.external.len() {
        if operation.external[index].state != ExternalState::Unknown {
            continue;
        }
        let request = request(operation, index, "status");
        let response = process::invoke(&adapter, &request);
        let state = match response.as_ref().map(|reply| reply.status.as_str()) {
            Ok("canceled" | "already_canceled") => ExternalState::Succeeded,
            Ok("pending") => ExternalState::Pending,
            _ => return Err(pending(operation, index)),
        };
        operation.external[index].state = state;
        save(context, ownership, operation)?;
    }
    if operation
        .external
        .iter()
        .all(|action| action.state == ExternalState::Succeeded)
    {
        complete(context, ownership, operation)?;
    }
    Ok(())
}

pub(super) fn cancel(
    context: &CommandContext,
    ownership: &OperationOwner,
    operation: &mut Operation,
) -> Result<(), Problem> {
    if operation
        .external
        .iter()
        .all(|action| action.state == ExternalState::Succeeded)
    {
        return complete(context, ownership, operation);
    }
    let adapter = adapter(context, ownership, operation)?;
    // No cancellation occurs until every remaining action passes preflight.
    for index in 0..operation.external.len() {
        match operation.external[index].state {
            ExternalState::Succeeded => continue,
            ExternalState::Unknown => return Err(pending(operation, index)),
            ExternalState::Pending => {}
        }
        let response = process::invoke(&adapter, &request(operation, index, "preflight"));
        if !matches!(
            response.as_ref().map(|reply| reply.status.as_str()),
            Ok("ready")
        ) {
            return Err(pending(operation, index));
        }
    }
    operation.phase = Phase::ExternalPending;
    save(context, ownership, operation)?;
    for index in 0..operation.external.len() {
        if operation.external[index].state == ExternalState::Succeeded {
            continue;
        }
        // Persist intent first: even a crash after remote success but before
        // acknowledgement must resume through status rather than blind retry.
        operation.external[index].state = ExternalState::Unknown;
        save(context, ownership, operation)?;
        let response = process::invoke(&adapter, &request(operation, index, "cancel"));
        if !matches!(
            response.as_ref().map(|reply| reply.status.as_str()),
            Ok("canceled" | "already_canceled")
        ) {
            return Err(pending(operation, index));
        }
        operation.external[index].state = ExternalState::Succeeded;
        save(context, ownership, operation)?;
    }
    complete(context, ownership, operation)
}

fn complete(
    context: &CommandContext,
    ownership: &OperationOwner,
    operation: &mut Operation,
) -> Result<(), Problem> {
    if operation.phase != Phase::ExternalComplete {
        operation.phase = Phase::ExternalComplete;
        save(context, ownership, operation)?;
    }
    Ok(())
}

fn adapter(
    context: &CommandContext,
    ownership: &OperationOwner,
    operation: &mut Operation,
) -> Result<AdapterSnapshot, Problem> {
    let result = (|| {
        let saved: IntegrationSnapshot =
            serde_json::from_value(operation.canonical_inputs["integration"].clone())
                .map_err(|error| operational("operation_unavailable", &error.into()))?;
        let current = config::load(context)?;
        if current != saved {
            return Err(problem(
                "stale_plan",
                "Removal integration changed; preview and confirm again",
            ));
        }
        current
            .adapter
            .ok_or_else(|| problem("adapter_unavailable", "Removal adapter is unavailable"))
    })();
    if result.is_err() {
        operation.phase = Phase::NeedsReconfirmation;
        save(context, ownership, operation)?;
    }
    result.map_err(|error| identified(operation, error))
}

fn request(operation: &Operation, index: usize, action: &str) -> Request {
    Request {
        schema_version: 1,
        operation_id: operation.operation_id.clone(),
        action_id: operation.external[index].action_id.clone(),
        workspace_id: operation.workspace_uuid.clone(),
        actor_id: operation.actor_id.clone(),
        action: action.into(),
        issue: operation.external[index].issue.clone(),
    }
}

fn pending(operation: &Operation, index: usize) -> Problem {
    Problem::new(
        "external_pending",
        "External removal is unfinished; inspect or resume the saved operation",
        serde_json::json!({"operation_id": operation.operation_id, "action_id": operation.external[index].action_id}),
    )
}

pub(super) fn identified(operation: &Operation, mut error: Problem) -> Problem {
    error.details["operation_id"] = serde_json::json!(operation.operation_id);
    error
}

pub(super) fn save(
    context: &CommandContext,
    ownership: &OperationOwner,
    operation: &Operation,
) -> Result<(), Problem> {
    ownership
        .save(&context.workspace, operation)
        .map_err(|error| identified(operation, operational("operation_unavailable", &error)))
}
