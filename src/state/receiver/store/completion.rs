use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::{to_i64, validated_owner};
use crate::state::{Db, ReceiverCompletionRequest, ReceiverObservationSet};

struct StoredEvidence {
    accepted: Option<i64>,
    progressing: Option<i64>,
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
                        accepted: row.get(0)?,
                        progressing: row.get(1)?,
                        completed: row.get(2)?,
                        revision: row.get(3)?,
                        session_id: row.get(4)?,
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
                 observation_revision = ?7, observation_session_id = ?8,
                 claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 updated_at_unix_ms = ?6
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3 AND claim_owner = ?9
               AND claim_expires_at_unix_ms > ?10
               AND observation_instance = ?11 AND state IN ('launched', 'accepted', 'processing')
               AND conversation_id = ?12",
            rusqlite::params![
                self.workspace_id,
                request.job_id.to_string(),
                request.token.to_string(),
                merged.accepted,
                merged.progressing,
                merged.completed,
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
    accepted: Option<i64>,
    progressing: Option<i64>,
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
    validate_timeline(stored.accepted, stored.progressing, stored.completed)?;
    anyhow::ensure!(
        stored.completed.is_none(),
        "receiver job is already completed"
    );
    let Some(observation) = observation else {
        return Ok(MergedEvidence {
            accepted: stored.accepted,
            progressing: stored.progressing,
            completed: latest_boundary(local_completion, stored.accepted, stored.progressing),
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
    let accepted = merge_boundary(stored.accepted, accepted, "accepted")?;
    let progressing = merge_boundary(stored.progressing, progressing, "progressing")?;
    validate_timeline(accepted, progressing, completed)?;
    let completed =
        completed.unwrap_or_else(|| latest_boundary(local_completion, accepted, progressing));
    anyhow::ensure!(
        accepted.is_none_or(|accepted| accepted <= completed)
            && progressing.is_none_or(|progressing| progressing <= completed),
        "receiver completion precedes durable lifecycle evidence"
    );
    Ok(MergedEvidence {
        accepted,
        progressing,
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
