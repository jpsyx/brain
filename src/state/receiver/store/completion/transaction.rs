use anyhow::{Context as _, Result};
use rusqlite::OptionalExtension as _;

use crate::state::{
    Db, ReceiverCompletionOutcome, ReceiverCompletionRequest, ReceiverDeliveryId,
    ReceiverObservationSet, render_receiver_transcript,
};

use super::authorization::{AuthorizedCompletion, CompletionEvidence, validate_inbound_scope};
use super::duplicate::existing_completion_outcome;
use super::lifecycle::{StoredEvidence, merge_completion_evidence};
use super::preparation;

struct ActiveCompletion {
    evidence: StoredEvidence,
    inbound: crate::server::receiver::InboundJob,
    transcript: String,
}

pub(super) fn complete(
    db: &Db,
    request: &ReceiverCompletionRequest<'_>,
    observation: Option<&ReceiverObservationSet>,
    authorized: &AuthorizedCompletion<'_>,
) -> Result<Option<ReceiverCompletionOutcome>> {
    let transaction =
        rusqlite::Transaction::new_unchecked(&db.conn, rusqlite::TransactionBehavior::Immediate)?;
    if let Some(outcome) =
        existing_completion_outcome(&transaction, &db.workspace_id, request, observation)?
    {
        return Ok(Some(outcome));
    }
    let scope = request.registration.scope();
    let stored = transaction
        .query_row(
            "SELECT accepted_at_unix_ms, progressing_at_unix_ms,
                    attempt_accepted_at_unix_ms, attempt_progressing_at_unix_ms,
                    latest_progress_at_unix_ms, completed_at_unix_ms,
                    observation_revision, observation_session_id,
                    job.inbound_json, job.response_sender,
                    conversation.transcript_markdown
             FROM receiver_jobs AS job
             JOIN receiver_conversations AS conversation
               ON conversation.workspace_id = job.workspace_id
              AND conversation.conversation_id = job.conversation_id
             WHERE job.workspace_id = ?1 AND job.job_id = ?2 AND job.job_token = ?3
               AND job.claim_owner = ?4 AND job.claim_expires_at_unix_ms > ?5
               AND job.observation_instance = ?6 AND job.conversation_id = ?7
               AND job.channel = ?8 AND conversation.user_id = ?9
               AND conversation.channel = ?8
               AND job.state IN ('launched', 'accepted', 'processing')",
            rusqlite::params![
                db.workspace_id,
                request.job_id.to_string(),
                request.token.to_string(),
                authorized.owner,
                authorized.authorized_at_unix_ms,
                request.registration.instance(),
                request.registration.conversation_id().to_string(),
                scope.actor().channel().as_str(),
                scope.actor().user_id().as_str(),
            ],
            |row| {
                Ok(ActiveCompletion {
                    evidence: StoredEvidence {
                        lifetime_accepted: row.get(0)?,
                        lifetime_progressing: row.get(1)?,
                        attempt_accepted: row.get(2)?,
                        attempt_progressing: row.get(3)?,
                        latest_progress: row.get(4)?,
                        completed: row.get(5)?,
                        revision: row.get(6)?,
                        session_id: row.get(7)?,
                    },
                    inbound: super::super::decode_inbound(
                        &row.get::<_, String>(8)?,
                        row.get::<_, Option<String>>(9)?,
                    )
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            8,
                            rusqlite::types::Type::Text,
                            error.into(),
                        )
                    })?,
                    transcript: row.get(10)?,
                })
            },
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    validate_inbound_scope(&stored.inbound, request)?;
    let prepared = preparation::prepare(&stored.inbound, request.answer)?;
    let envelope_json = prepared.envelope_json();
    let transcript =
        render_receiver_transcript(&stored.transcript, &stored.inbound.prompt, request.answer);
    let merged = merge_completion_evidence(
        &stored.evidence,
        observation,
        request,
        authorized.observed_at_unix_ms,
    )?;
    let completion_evidence = CompletionEvidence::new(
        &db.workspace_id,
        request,
        &stored.inbound.prompt,
        envelope_json,
        &merged,
    );
    let completion_evidence_json = serde_json::to_string(&completion_evidence)
        .context("serialize durable receiver completion evidence")?;
    if !super::super::session::replace_receiver_binding_in_transaction(
        &transaction,
        &db.workspace_id,
        request.registration,
        super::super::session::ReceiverBindingTarget::ExactCompleted(request.completed_session),
        u64::try_from(merged.completed)
            .map_err(|_| anyhow::anyhow!("receiver completion time is negative"))?,
    )? {
        return Ok(None);
    }
    let conversation_changed = transaction.execute(
        "UPDATE receiver_conversations
         SET transcript_markdown = ?8, updated_at_unix_ms = ?9
         WHERE workspace_id = ?1 AND conversation_id = ?2
           AND user_id = ?3 AND channel = ?4
           AND agent_kind = ?5 AND agent_session_id = ?6
           AND transcript_markdown = ?7",
        rusqlite::params![
            db.workspace_id,
            request.registration.conversation_id().to_string(),
            scope.actor().user_id().as_str(),
            scope.actor().channel().as_str(),
            scope.agent_kind().as_str(),
            request.completed_session.as_str(),
            stored.transcript,
            transcript,
            merged.completed,
        ],
    )?;
    if conversation_changed != 1 {
        return Ok(None);
    }
    let delivery_id = ReceiverDeliveryId::new();
    transaction.execute(
        "INSERT INTO receiver_deliveries
           (delivery_id, job_id, job_token, response_kind, envelope_json,
            completion_evidence_json, state, attempt_count, error_category,
            fallback_decision, created_at_unix_ms, updated_at_unix_ms)
         VALUES (?1, ?2, ?3, 'final-answer', ?4, ?5, ?6, 0, ?7, ?8, ?9, ?9)",
        rusqlite::params![
            delivery_id.to_string(),
            request.job_id.to_string(),
            request.token.to_string(),
            envelope_json,
            completion_evidence_json,
            prepared.state(),
            prepared.error_category(),
            (prepared.state() == "failed").then_some("no-safe-fallback"),
            merged.completed,
        ],
    )?;
    transaction.execute(
        "INSERT INTO receiver_answer_cleanups
           (job_id, job_token, workspace_id, conversation_id, brain_instance_id,
            agent_kind, actor_id, channel, registered_session_id, actual_session_id,
            controller_shutdown_acknowledged, session_released, artifacts_removed,
            created_at_unix_ms, updated_at_unix_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, 0, 0, ?11, ?11)",
        rusqlite::params![
            request.job_id.to_string(),
            request.token.to_string(),
            db.workspace_id,
            request.registration.conversation_id().to_string(),
            request.registration.instance(),
            scope.agent_kind().as_str(),
            scope.actor().user_id().as_str(),
            scope.actor().channel().as_str(),
            request.registration.registered_session().as_str(),
            request.completed_session.as_str(),
            merged.completed,
        ],
    )?;
    let changed = transaction.execute(
        "UPDATE receiver_jobs
         SET state = ?16, accepted_at_unix_ms = ?4,
             progressing_at_unix_ms = ?5, completed_at_unix_ms = ?6,
             attempt_accepted_at_unix_ms = ?7,
             attempt_progressing_at_unix_ms = ?8,
             latest_progress_at_unix_ms = COALESCE(?9, latest_progress_at_unix_ms),
             observation_revision = ?10, observation_session_id = ?11,
             claim_owner = NULL, claim_expires_at_unix_ms = NULL,
             retry_at_unix_ms = NULL, retry_from_state = NULL,
             last_error = ?17,
             updated_at_unix_ms = ?6
         WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3 AND claim_owner = ?12
           AND claim_expires_at_unix_ms > ?13
           AND observation_instance = ?14 AND state IN ('launched', 'accepted', 'processing')
           AND conversation_id = ?15",
        rusqlite::params![
            db.workspace_id,
            request.job_id.to_string(),
            request.token.to_string(),
            merged.lifetime_accepted,
            merged.lifetime_progressing,
            merged.completed,
            merged.attempt_accepted,
            merged.attempt_progressing,
            merged.latest_progress,
            merged.revision,
            merged.session_id,
            authorized.owner,
            authorized.authorized_at_unix_ms,
            request.registration.instance(),
            request.registration.conversation_id().to_string(),
            prepared.job_state(),
            prepared.job_error(),
        ],
    )?;
    if changed != 1 {
        return Ok(None);
    }
    transaction.commit()?;
    Ok(Some(ReceiverCompletionOutcome::recorded(delivery_id)))
}
