use anyhow::Result;
use rusqlite::OptionalExtension as _;

use crate::state::{
    ReceiverCompletionOutcome, ReceiverCompletionRequest, ReceiverDeliveryEnvelope,
    ReceiverDeliveryId, ReceiverJobState, ReceiverObservationSet,
};

use super::authorization::CompletionEvidence;
use super::lifecycle::validate_existing_observation;

struct ExistingCompletion {
    delivery_id: String,
    delivery_token: String,
    envelope_json: String,
    completion_evidence_json: Option<String>,
    job_state: String,
}

pub(super) fn existing_completion_outcome(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    request: &ReceiverCompletionRequest<'_>,
    observation: Option<&ReceiverObservationSet>,
) -> Result<Option<ReceiverCompletionOutcome>> {
    let stored = transaction
        .query_row(
            "SELECT delivery.delivery_id, delivery.job_token, delivery.envelope_json,
                    delivery.completion_evidence_json, job.state
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
             WHERE job.workspace_id = ?1 AND job.job_id = ?2
               AND delivery.response_kind = 'final-answer'",
            rusqlite::params![workspace_id, request.job_id.to_string()],
            |row| {
                Ok(ExistingCompletion {
                    delivery_id: row.get(0)?,
                    delivery_token: row.get(1)?,
                    envelope_json: row.get(2)?,
                    completion_evidence_json: row.get(3)?,
                    job_state: row.get(4)?,
                })
            },
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    let exact_state = matches!(
        ReceiverJobState::parse(&stored.job_state),
        Some(
            ReceiverJobState::AnswerReady
                | ReceiverJobState::Delivering
                | ReceiverJobState::Retrying
                | ReceiverJobState::Failed
                | ReceiverJobState::Done
        )
    );
    anyhow::ensure!(
        stored.delivery_token == request.token.to_string() && exact_state,
        "receiver completion conflicts with durable answer"
    );
    let completion_evidence_json = stored
        .completion_evidence_json
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("receiver completion conflicts with durable answer"))?;
    let completion_evidence: CompletionEvidence = serde_json::from_str(completion_evidence_json)
        .map_err(|_| anyhow::anyhow!("receiver completion evidence is invalid"))?;
    let _: ReceiverDeliveryEnvelope = serde_json::from_str(&stored.envelope_json)
        .map_err(|_| anyhow::anyhow!("receiver delivery envelope is invalid"))?;
    anyhow::ensure!(
        completion_evidence.matches(workspace_id, request, &stored.envelope_json),
        "receiver completion conflicts with durable answer"
    );
    validate_existing_observation(&completion_evidence.stored_evidence(), observation, request)?;
    Ok(Some(ReceiverCompletionOutcome::existing(
        ReceiverDeliveryId::parse(&stored.delivery_id)?,
    )))
}
