use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::{to_i64, validated_owner};
use crate::state::{Db, ReceiverCompletionRequest, ReceiverObservationSet};

struct StoredEvidence {
    lifetime_accepted: Option<i64>,
    lifetime_progressing: Option<i64>,
    attempt_accepted: Option<i64>,
    attempt_progressing: Option<i64>,
    completed: Option<i64>,
    revision: i64,
    session_id: Option<String>,
}

impl Db {
    /// Commit the exact native binding and terminal job transition together.
    pub fn complete_receiver_job_with_binding(
        &self,
        request: &ReceiverCompletionRequest<'_>,
    ) -> Result<bool> {
        self.complete_receiver_job_with_observation(request, None)
    }

    /// Commit optional normalized lifecycle evidence, completion, and binding together.
    pub fn complete_receiver_job_with_observation(
        &self,
        request: &ReceiverCompletionRequest<'_>,
        observation: Option<&ReceiverObservationSet>,
    ) -> Result<bool> {
        let owner = validated_owner(request.owner)?;
        anyhow::ensure!(
            request.registration.scope().workspace_id().to_string() == self.workspace_id,
            "receiver session scope belongs to another workspace"
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
        let stored = transaction
            .query_row(
                "SELECT accepted_at_unix_ms, progressing_at_unix_ms,
                        attempt_accepted_at_unix_ms, attempt_progressing_at_unix_ms,
                        completed_at_unix_ms, observation_revision, observation_session_id
                 FROM receiver_jobs
                 WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
                   AND claim_owner = ?4 AND claim_expires_at_unix_ms > ?5
                   AND observation_instance = ?6 AND conversation_id = ?7
                   AND state IN ('launched', 'accepted', 'processing')",
                rusqlite::params![
                    self.workspace_id,
                    request.job_id.to_string(),
                    request.token.to_string(),
                    owner,
                    authorized,
                    request.registration.instance(),
                    request.registration.conversation_id().to_string(),
                ],
                |row| {
                    Ok(StoredEvidence {
                        lifetime_accepted: row.get(0)?,
                        lifetime_progressing: row.get(1)?,
                        attempt_accepted: row.get(2)?,
                        attempt_progressing: row.get(3)?,
                        completed: row.get(4)?,
                        revision: row.get(5)?,
                        session_id: row.get(6)?,
                    })
                },
            )
            .optional()?;
        let Some(stored) = stored else {
            return Ok(false);
        };
        let merged = merge_completion_evidence(&stored, observation, request, observed)?;
        if !super::session::replace_receiver_binding_in_transaction(
            &transaction,
            &self.workspace_id,
            request.registration,
            super::session::ReceiverBindingTarget::ExactCompleted(request.completed_session),
            u64::try_from(merged.completed)
                .map_err(|_| anyhow::anyhow!("receiver completion time is negative"))?,
        )? {
            return Ok(false);
        }
        let changed = transaction.execute(
            "UPDATE receiver_jobs
             SET state = 'done', accepted_at_unix_ms = ?4,
                 progressing_at_unix_ms = ?5, completed_at_unix_ms = ?6,
                 attempt_accepted_at_unix_ms = ?7,
                 attempt_progressing_at_unix_ms = ?8,
                 latest_progress_at_unix_ms = COALESCE(?8, latest_progress_at_unix_ms),
                 observation_revision = ?9, observation_session_id = ?10,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 updated_at_unix_ms = ?6
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3 AND claim_owner = ?11
               AND claim_expires_at_unix_ms > ?12
               AND observation_instance = ?13 AND state IN ('launched', 'accepted', 'processing')
               AND conversation_id = ?14",
            rusqlite::params![
                self.workspace_id,
                request.job_id.to_string(),
                request.token.to_string(),
                merged.lifetime_accepted,
                merged.lifetime_progressing,
                merged.completed,
                merged.attempt_accepted,
                merged.attempt_progressing,
                merged.revision,
                merged.session_id,
                owner,
                authorized,
                request.registration.instance(),
                request.registration.conversation_id().to_string(),
            ],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }
}

struct MergedEvidence {
    lifetime_accepted: Option<i64>,
    lifetime_progressing: Option<i64>,
    attempt_accepted: Option<i64>,
    attempt_progressing: Option<i64>,
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
            completed: latest_boundary(
                local_completion,
                stored.attempt_accepted,
                stored.attempt_progressing,
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
    let completed = observation
        .completed_at_unix_ms
        .map(|value| to_i64(value, "receiver completed observation time"))
        .transpose()?;
    validate_timeline(accepted, progressing, completed)?;
    let attempt_accepted = merge_boundary(stored.attempt_accepted, accepted, "accepted")?;
    let attempt_progressing =
        merge_boundary(stored.attempt_progressing, progressing, "progressing")?;
    validate_timeline(attempt_accepted, attempt_progressing, completed)?;
    let completed = completed.unwrap_or_else(|| {
        latest_boundary(local_completion, attempt_accepted, attempt_progressing)
    });
    anyhow::ensure!(
        attempt_accepted.is_none_or(|accepted| accepted <= completed)
            && attempt_progressing.is_none_or(|progressing| progressing <= completed),
        "receiver completion precedes durable lifecycle evidence"
    );
    Ok(MergedEvidence {
        lifetime_accepted: stored.lifetime_accepted.or(attempt_accepted),
        lifetime_progressing: stored.lifetime_progressing.or(attempt_progressing),
        attempt_accepted,
        attempt_progressing,
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
