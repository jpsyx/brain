use anyhow::Result;

use super::{to_i64, validated_owner};
use crate::agent::AgentSession;
use crate::state::{
    Db, ReceiverCompletionRequest, ReceiverJobId, ReceiverLaunchObservation,
    ReceiverLifecycleDeadlines, ReceiverNonterminalObservationPhase, ReceiverObservation,
    ReceiverObservationSet, ReceiverSessionAttribution, receiver_acceptance_expires_at,
};

impl Db {
    /// Commit post-spawn evidence only for the exact live pre-spawn owner.
    pub fn commit_receiver_job_launch(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        observation: &ReceiverLaunchObservation,
    ) -> Result<bool> {
        let owner = validated_owner(owner)?;
        let instance = validated_owner(&observation.instance)?;
        let session_id = validated_owner(&observation.session_id)?;
        let observed = to_i64(
            observation.observed_at_unix_ms,
            "receiver launched observation time",
        )?;
        let authorized = to_i64(
            observation.authorized_at_unix_ms,
            "receiver launch authorization time",
        )?;
        let acceptance_expires = to_i64(
            receiver_acceptance_expires_at(observation.authorized_at_unix_ms),
            "receiver acceptance expiry",
        )?;
        Ok(self.conn.execute(
            "UPDATE receiver_jobs
             SET state = 'launched', launched_at_unix_ms = ?6,
                 observation_instance = ?4, observation_session_id = ?5,
                 observation_revision = 0, acceptance_expires_at_unix_ms = ?9,
                 updated_at_unix_ms = ?6
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND claim_owner = ?7 AND claim_expires_at_unix_ms > ?8
               AND state = 'launching'",
            rusqlite::params![
                self.workspace_id,
                job_id.to_string(),
                observation.token.to_string(),
                instance,
                session_id,
                observed,
                owner,
                authorized,
                acceptance_expires,
            ],
        )? == 1)
    }

    /// Apply one newer token-matched receiver observation without inventing prior facts.
    pub fn apply_receiver_observation(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        observation: &ReceiverObservation,
    ) -> Result<bool> {
        let mut set = ReceiverObservationSet {
            token: observation.token,
            instance: observation.instance.clone(),
            session_id: observation.session_id.clone(),
            revision: observation.revision,
            accepted_at_unix_ms: None,
            progressing_at_unix_ms: None,
            completed_at_unix_ms: None,
            authorized_at_unix_ms: observation.authorized_at_unix_ms,
        };
        match observation.phase {
            ReceiverNonterminalObservationPhase::Accepted => {
                set.accepted_at_unix_ms = Some(observation.observed_at_unix_ms);
            }
            ReceiverNonterminalObservationPhase::Progressing => {
                set.progressing_at_unix_ms = Some(observation.observed_at_unix_ms);
            }
        }
        self.apply_receiver_observation_set(job_id, owner, &set)
    }

    /// Apply all newly represented boundaries from one newer snapshot atomically.
    pub fn apply_receiver_observation_set(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        observation: &ReceiverObservationSet,
    ) -> Result<bool> {
        let owner = validated_owner(owner)?;
        let instance = validated_owner(&observation.instance)?;
        let session_id = validated_owner(&observation.session_id)?;
        let revision = to_i64(observation.revision, "receiver observation revision")?;
        let authorized = to_i64(
            observation.authorized_at_unix_ms,
            "receiver observation authorization time",
        )?;
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
        let deadlines = ReceiverLifecycleDeadlines::after_acceptance(
            observation.authorized_at_unix_ms,
            observation
                .accepted_at_unix_ms
                .unwrap_or(observation.authorized_at_unix_ms),
        );
        let progress_expires = to_i64(
            deadlines.progress_expires_at_unix_ms,
            "receiver progress expiry",
        )?;
        let absolute_work_expires = observation
            .accepted_at_unix_ms
            .map(|_| {
                to_i64(
                    deadlines.absolute_work_expires_at_unix_ms,
                    "receiver absolute-work expiry",
                )
            })
            .transpose()?;
        anyhow::ensure!(
            accepted.is_some() || progressing.is_some() || completed.is_some(),
            "receiver observation set cannot be empty"
        );
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
        anyhow::ensure!(
            completed.is_none(),
            "terminal receiver observation requires exact binding authorization"
        );
        let (next, states) = if progressing.is_some() {
            (
                "processing",
                if accepted.is_some() {
                    "'launched', 'accepted'"
                } else {
                    "'accepted'"
                },
            )
        } else {
            ("accepted", "'launched'")
        };
        let sql = format!(
            "UPDATE receiver_jobs SET state = ?8,
                 accepted_at_unix_ms = COALESCE(accepted_at_unix_ms, ?10),
                 progressing_at_unix_ms = COALESCE(progressing_at_unix_ms, ?11),
                 attempt_accepted_at_unix_ms = COALESCE(attempt_accepted_at_unix_ms, ?10),
                 attempt_progressing_at_unix_ms = COALESCE(attempt_progressing_at_unix_ms, ?11),
                 latest_progress_at_unix_ms = COALESCE(?11, latest_progress_at_unix_ms),
                 progress_expires_at_unix_ms = MIN(
                   ?13, COALESCE(absolute_work_expires_at_unix_ms, ?14, ?13)
                 ),
                 absolute_work_expires_at_unix_ms = COALESCE(
                   absolute_work_expires_at_unix_ms, ?14
                 ),
                 observation_revision = ?6, observation_session_id = ?9,
                 updated_at_unix_ms = ?7
                 WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3 AND claim_owner = ?4
                   AND claim_expires_at_unix_ms > ?12 AND observation_instance = ?5
                   AND (observation_revision = 0 OR observation_session_id = ?9)
                   AND EXISTS(
                     SELECT 1 FROM brain_sessions AS active
                     JOIN receiver_session_registrations AS registration
                       ON registration.workspace_id = active.workspace_id
                      AND registration.brain_instance_id = active.brain_instance_id
                      AND registration.agent_kind = active.agent_kind
                      AND registration.actor_id = active.actor_id
                      AND registration.channel = active.channel
                     WHERE active.workspace_id = ?1 AND active.brain_instance_id = ?5
                       AND active.agent_session_id = ?9 AND active.locked_pid IS NOT NULL
                       AND registration.conversation_id = receiver_jobs.conversation_id
                   )
                   AND observation_revision < ?6 AND state IN ({states})"
        );
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let changed = transaction.execute(
            &sql,
            rusqlite::params![
                self.workspace_id,
                job_id.to_string(),
                observation.token.to_string(),
                owner,
                instance,
                revision,
                authorized,
                next,
                session_id,
                accepted,
                progressing,
                authorized,
                progress_expires,
                absolute_work_expires,
            ],
        )?;
        if changed == 1 {
            transaction.commit()?;
        }
        Ok(changed == 1)
    }

    /// Commit terminal lifecycle evidence and its exact native binding together.
    pub fn apply_terminal_receiver_observation_set(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        observation: &ReceiverObservationSet,
        registration: &ReceiverSessionAttribution,
        completed_session: &AgentSession,
    ) -> Result<bool> {
        let completed_at_unix_ms = observation
            .completed_at_unix_ms
            .ok_or_else(|| anyhow::anyhow!("terminal receiver observation is incomplete"))?;
        self.complete_receiver_job_with_observation(
            &ReceiverCompletionRequest {
                job_id,
                token: observation.token,
                owner,
                registration,
                completed_session,
                observed_at_unix_ms: completed_at_unix_ms,
                authorized_at_unix_ms: observation.authorized_at_unix_ms,
            },
            Some(observation),
        )
    }
}
