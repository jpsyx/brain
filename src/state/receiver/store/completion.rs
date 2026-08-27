use anyhow::{Context as _, Result};
use rusqlite::OptionalExtension as _;

use super::{to_i64, validated_owner};
use crate::state::{
    Db, ReceiverCompletionOutcome, ReceiverCompletionRequest, ReceiverDeliveryEnvelope,
    ReceiverDeliveryId, ReceiverJobState, ReceiverObservationSet, ReceiverResponseKind,
    render_receiver_delivery, render_receiver_transcript,
};

struct StoredEvidence {
    lifetime_accepted: Option<i64>,
    lifetime_progressing: Option<i64>,
    attempt_accepted: Option<i64>,
    attempt_progressing: Option<i64>,
    latest_progress: Option<i64>,
    completed: Option<i64>,
    revision: i64,
    session_id: Option<String>,
}

struct ActiveCompletion {
    evidence: StoredEvidence,
    inbound: crate::server::receiver::InboundJob,
    transcript: String,
}

struct ExistingCompletion {
    delivery_id: String,
    delivery_token: String,
    envelope_json: String,
    job_state: String,
    conversation_id: String,
    observation_instance: Option<String>,
    observation_session_id: Option<String>,
    inbound_json: String,
    transcript: String,
    actor_id: String,
    channel: String,
    agent_kind: Option<String>,
    native_session_id: Option<String>,
    evidence: StoredEvidence,
}

impl Db {
    /// Commit the exact native binding, transcript, answer outbox, and answer-ready transition.
    pub fn complete_receiver_job_with_binding(
        &self,
        request: &ReceiverCompletionRequest<'_>,
    ) -> Result<Option<ReceiverCompletionOutcome>> {
        self.complete_receiver_job_with_observation(request, None)
    }

    /// Commit optional lifecycle evidence and one exact durable answer atomically.
    pub fn complete_receiver_job_with_observation(
        &self,
        request: &ReceiverCompletionRequest<'_>,
        observation: Option<&ReceiverObservationSet>,
    ) -> Result<Option<ReceiverCompletionOutcome>> {
        let owner = validated_owner(request.owner)?;
        anyhow::ensure!(
            request.registration.scope().workspace_id().to_string() == self.workspace_id,
            "receiver session scope belongs to another workspace"
        );
        anyhow::ensure!(
            !request.answer.trim().is_empty(),
            "receiver completion answer cannot be blank"
        );
        anyhow::ensure!(
            request.answer.len() <= crate::state::MAX_RECEIVER_ANSWER_BYTES,
            "receiver completion answer is too large"
        );
        let observed = to_i64(request.observed_at_unix_ms, "receiver completion time")?;
        let authorized = to_i64(
            request.authorized_at_unix_ms,
            "receiver completion authorization time",
        )?;
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if let Some(outcome) =
            existing_completion_outcome(&transaction, &self.workspace_id, request, observation)?
        {
            return Ok(Some(outcome));
        }
        let scope = request.registration.scope();
        let stored = transaction
            .query_row(
                "SELECT accepted_at_unix_ms, progressing_at_unix_ms,
                        attempt_accepted_at_unix_ms, attempt_progressing_at_unix_ms,
                        latest_progress_at_unix_ms, completed_at_unix_ms,
                        observation_revision, observation_session_id
                        , job.inbound_json, conversation.transcript_markdown
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
                    self.workspace_id,
                    request.job_id.to_string(),
                    request.token.to_string(),
                    owner,
                    authorized,
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
                        inbound: serde_json::from_str::<crate::server::receiver::InboundJob>(
                            &row.get::<_, String>(8)?,
                        )
                        .map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                8,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })?,
                        transcript: row.get(9)?,
                    })
                },
            )
            .optional()?;
        let Some(stored) = stored else {
            return Ok(None);
        };
        validate_inbound_scope(&stored.inbound, request)?;
        let envelope = render_receiver_delivery(
            &stored.inbound,
            ReceiverResponseKind::FinalAnswer,
            request.answer,
        )?;
        let envelope_json = serde_json::to_string(&envelope)
            .context("serialize durable receiver delivery envelope")?;
        let transcript =
            render_receiver_transcript(&stored.transcript, &stored.inbound.prompt, request.answer);
        let merged = merge_completion_evidence(&stored.evidence, observation, request, observed)?;
        if !super::session::replace_receiver_binding_in_transaction(
            &transaction,
            &self.workspace_id,
            request.registration,
            super::session::ReceiverBindingTarget::ExactCompleted(request.completed_session),
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
                self.workspace_id,
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
                state, attempt_count, created_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, ?2, ?3, 'final-answer', ?4, 'ready', 0, ?5, ?5)",
            rusqlite::params![
                delivery_id.to_string(),
                request.job_id.to_string(),
                request.token.to_string(),
                envelope_json,
                merged.completed,
            ],
        )?;
        let changed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'answer-ready', accepted_at_unix_ms = ?4,
                 progressing_at_unix_ms = ?5, completed_at_unix_ms = ?6,
                 attempt_accepted_at_unix_ms = ?7,
                 attempt_progressing_at_unix_ms = ?8,
                 latest_progress_at_unix_ms = COALESCE(?9, latest_progress_at_unix_ms),
                 observation_revision = ?10, observation_session_id = ?11,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 last_error = NULL, pending_unavailable_notice = 0,
                 updated_at_unix_ms = ?6
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3 AND claim_owner = ?12
               AND claim_expires_at_unix_ms > ?13
               AND observation_instance = ?14 AND state IN ('launched', 'accepted', 'processing')
               AND conversation_id = ?15",
            rusqlite::params![
                self.workspace_id,
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
                owner,
                authorized,
                request.registration.instance(),
                request.registration.conversation_id().to_string(),
            ],
        )?;
        if changed != 1 {
            return Ok(None);
        }
        transaction.commit()?;
        Ok(Some(ReceiverCompletionOutcome::recorded(delivery_id)))
    }
}

fn existing_completion_outcome(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    request: &ReceiverCompletionRequest<'_>,
    observation: Option<&ReceiverObservationSet>,
) -> Result<Option<ReceiverCompletionOutcome>> {
    let stored = transaction
        .query_row(
            "SELECT delivery.delivery_id, delivery.job_token, delivery.envelope_json,
                    job.state, job.conversation_id, job.observation_instance,
                    job.observation_session_id, job.inbound_json,
                    conversation.transcript_markdown, conversation.user_id,
                    conversation.channel, conversation.agent_kind,
                    conversation.agent_session_id,
                    job.accepted_at_unix_ms, job.progressing_at_unix_ms,
                    job.attempt_accepted_at_unix_ms,
                    job.attempt_progressing_at_unix_ms,
                    job.latest_progress_at_unix_ms, job.completed_at_unix_ms,
                    job.observation_revision, job.observation_session_id
             FROM receiver_deliveries AS delivery
             JOIN receiver_jobs AS job ON job.job_id = delivery.job_id
             JOIN receiver_conversations AS conversation
               ON conversation.workspace_id = job.workspace_id
              AND conversation.conversation_id = job.conversation_id
             WHERE job.workspace_id = ?1 AND job.job_id = ?2
               AND delivery.response_kind = 'final-answer'",
            rusqlite::params![workspace_id, request.job_id.to_string()],
            |row| {
                Ok(ExistingCompletion {
                    delivery_id: row.get(0)?,
                    delivery_token: row.get(1)?,
                    envelope_json: row.get(2)?,
                    job_state: row.get(3)?,
                    conversation_id: row.get(4)?,
                    observation_instance: row.get(5)?,
                    observation_session_id: row.get(6)?,
                    inbound_json: row.get(7)?,
                    transcript: row.get(8)?,
                    actor_id: row.get(9)?,
                    channel: row.get(10)?,
                    agent_kind: row.get(11)?,
                    native_session_id: row.get(12)?,
                    evidence: StoredEvidence {
                        lifetime_accepted: row.get(13)?,
                        lifetime_progressing: row.get(14)?,
                        attempt_accepted: row.get(15)?,
                        attempt_progressing: row.get(16)?,
                        latest_progress: row.get(17)?,
                        completed: row.get(18)?,
                        revision: row.get(19)?,
                        session_id: row.get(20)?,
                    },
                })
            },
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    let scope = request.registration.scope();
    let exact_identity = stored.delivery_token == request.token.to_string()
        && stored.conversation_id == request.registration.conversation_id().to_string()
        && stored.observation_instance.as_deref() == Some(request.registration.instance())
        && stored.observation_session_id.as_deref() == Some(request.completed_session.as_str())
        && stored.actor_id == scope.actor().user_id().as_str()
        && stored.channel == scope.actor().channel().as_str()
        && stored.agent_kind.as_deref() == Some(scope.agent_kind().as_str())
        && stored.native_session_id.as_deref() == Some(request.completed_session.as_str())
        && matches!(
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
        exact_identity,
        "receiver completion conflicts with durable answer"
    );
    let inbound: crate::server::receiver::InboundJob =
        serde_json::from_str(&stored.inbound_json).context("parse durable receiver answer job")?;
    validate_inbound_scope(&inbound, request)?;
    let expected_envelope =
        render_receiver_delivery(&inbound, ReceiverResponseKind::FinalAnswer, request.answer)?;
    let stored_envelope: ReceiverDeliveryEnvelope = serde_json::from_str(&stored.envelope_json)
        .context("parse durable receiver delivery envelope")?;
    anyhow::ensure!(
        stored_envelope == expected_envelope
            && crate::state::receiver_transcript_has_exact_turn(
                &stored.transcript,
                &inbound.prompt,
                request.answer,
            ),
        "receiver completion conflicts with durable answer"
    );
    validate_existing_observation(&stored.evidence, observation, request)?;
    Ok(Some(ReceiverCompletionOutcome::existing(
        ReceiverDeliveryId::parse(&stored.delivery_id)?,
    )))
}

fn validate_inbound_scope(
    inbound: &crate::server::receiver::InboundJob,
    request: &ReceiverCompletionRequest<'_>,
) -> Result<()> {
    let scope = request.registration.scope();
    anyhow::ensure!(
        inbound.workspace_id == scope.workspace_id()
            && inbound.actor.user_id() == scope.actor().user_id()
            && super::channel_str(inbound.channel) == scope.actor().channel().as_str(),
        "receiver completion scope conflicts with accepted job"
    );
    Ok(())
}

fn validate_existing_observation(
    stored: &StoredEvidence,
    observation: Option<&ReceiverObservationSet>,
    request: &ReceiverCompletionRequest<'_>,
) -> Result<()> {
    let Some(observation) = observation else {
        return Ok(());
    };
    let revision = to_i64(observation.revision, "receiver observation revision")?;
    let exact = observation.token == request.token
        && observation.instance == request.registration.instance()
        && observation.session_id == request.completed_session.as_str()
        && revision == stored.revision
        && observation
            .accepted_at_unix_ms
            .map(|value| to_i64(value, "receiver accepted observation time"))
            .transpose()?
            == stored.attempt_accepted
        && observation
            .progressing_at_unix_ms
            .map(|value| to_i64(value, "receiver progressing observation time"))
            .transpose()?
            == stored.attempt_progressing
        && observation
            .latest_progress_at_unix_ms
            .map(|value| to_i64(value, "receiver latest-progress observation time"))
            .transpose()?
            == stored.latest_progress
        && observation
            .completed_at_unix_ms
            .map(|value| to_i64(value, "receiver completed observation time"))
            .transpose()?
            == stored.completed;
    anyhow::ensure!(
        exact,
        "receiver completion observation conflicts with durable answer"
    );
    Ok(())
}

struct MergedEvidence {
    lifetime_accepted: Option<i64>,
    lifetime_progressing: Option<i64>,
    attempt_accepted: Option<i64>,
    attempt_progressing: Option<i64>,
    latest_progress: Option<i64>,
    completed: i64,
    revision: i64,
    session_id: Option<String>,
}

fn merge_completion_evidence(
    stored: &StoredEvidence,
    observation: Option<&ReceiverObservationSet>,
    request: &ReceiverCompletionRequest<'_>,
    local_completion: i64,
) -> Result<MergedEvidence> {
    validate_timeline(
        stored.lifetime_accepted,
        stored.lifetime_progressing,
        stored.completed,
    )?;
    validate_timeline(
        stored.attempt_accepted,
        stored.attempt_progressing,
        stored.completed,
    )?;
    anyhow::ensure!(
        stored.completed.is_none(),
        "receiver job is already completed"
    );
    let Some(observation) = observation else {
        return Ok(MergedEvidence {
            lifetime_accepted: stored.lifetime_accepted,
            lifetime_progressing: stored.lifetime_progressing,
            attempt_accepted: stored.attempt_accepted,
            attempt_progressing: stored.attempt_progressing,
            latest_progress: stored.latest_progress,
            completed: latest_boundary(
                local_completion,
                stored.attempt_accepted,
                stored.latest_progress.or(stored.attempt_progressing),
            ),
            revision: stored.revision.max(1),
            session_id: Some(request.completed_session.as_str().to_owned()),
        });
    };
    anyhow::ensure!(
        observation.token == request.token
            && observation.instance == request.registration.instance()
            && observation.session_id == request.completed_session.as_str(),
        "receiver completion observation identity mismatch"
    );
    let revision = to_i64(observation.revision, "receiver observation revision")?;
    anyhow::ensure!(
        revision > stored.revision,
        "receiver completion observation is not newer"
    );
    anyhow::ensure!(
        stored.revision == 0
            || stored.session_id.as_deref() == Some(observation.session_id.as_str()),
        "receiver observation session continuity mismatch"
    );
    let accepted = observation
        .accepted_at_unix_ms
        .map(|value| to_i64(value, "receiver accepted observation time"))
        .transpose()?;
    let progressing = observation
        .progressing_at_unix_ms
        .map(|value| to_i64(value, "receiver progressing observation time"))
        .transpose()?;
    let latest_progress = observation
        .latest_progress_at_unix_ms
        .map(|value| to_i64(value, "receiver latest-progress observation time"))
        .transpose()?;
    let completed = observation
        .completed_at_unix_ms
        .map(|value| to_i64(value, "receiver completed observation time"))
        .transpose()?;
    validate_timeline(accepted, progressing, completed)?;
    anyhow::ensure!(
        progressing.is_some() == latest_progress.is_some()
            && progressing
                .zip(latest_progress)
                .is_none_or(|(first, latest)| first <= latest)
            && latest_progress
                .zip(completed)
                .is_none_or(|(latest, completed)| latest <= completed),
        "receiver progress-pulse observation is inconsistent"
    );
    let attempt_accepted = merge_boundary(stored.attempt_accepted, accepted, "accepted")?;
    let attempt_progressing =
        merge_boundary(stored.attempt_progressing, progressing, "progressing")?;
    anyhow::ensure!(
        stored
            .latest_progress
            .zip(latest_progress)
            .is_none_or(|(stored, incoming)| stored <= incoming),
        "receiver latest-progress observation regressed"
    );
    let latest_progress = latest_progress.or(stored.latest_progress);
    validate_timeline(attempt_accepted, attempt_progressing, completed)?;
    let completed = completed
        .unwrap_or_else(|| latest_boundary(local_completion, attempt_accepted, latest_progress));
    anyhow::ensure!(
        attempt_accepted.is_none_or(|accepted| accepted <= completed)
            && attempt_progressing.is_none_or(|progressing| progressing <= completed)
            && latest_progress.is_none_or(|latest| latest <= completed),
        "receiver completion precedes durable lifecycle evidence"
    );
    Ok(MergedEvidence {
        lifetime_accepted: stored.lifetime_accepted.or(attempt_accepted),
        lifetime_progressing: stored.lifetime_progressing.or(attempt_progressing),
        attempt_accepted,
        attempt_progressing,
        latest_progress,
        completed,
        revision,
        session_id: Some(observation.session_id.clone()),
    })
}

fn merge_boundary(stored: Option<i64>, incoming: Option<i64>, label: &str) -> Result<Option<i64>> {
    anyhow::ensure!(
        stored
            .zip(incoming)
            .is_none_or(|(left, right)| left == right),
        "receiver {label} observation conflicts with durable evidence"
    );
    Ok(stored.or(incoming))
}

fn latest_boundary(local: i64, accepted: Option<i64>, progressing: Option<i64>) -> i64 {
    progressing
        .or(accepted)
        .map_or(local, |prior| local.max(prior))
}

fn validate_timeline(
    accepted: Option<i64>,
    progressing: Option<i64>,
    completed: Option<i64>,
) -> Result<()> {
    anyhow::ensure!(
        accepted
            .zip(progressing)
            .is_none_or(|(first, second)| first <= second)
            && accepted
                .zip(completed)
                .is_none_or(|(first, last)| first <= last)
            && progressing
                .zip(completed)
                .is_none_or(|(middle, last)| middle <= last),
        "receiver observation timestamps are not ordered"
    );
    Ok(())
}
