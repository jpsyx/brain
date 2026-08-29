use anyhow::Result;
use rusqlite::OptionalExtension as _;

use super::{to_i64, validated_owner};
use crate::state::{
    Db, ReceiverJobId, ReceiverLaunchObservation, ReceiverLifecycleDeadlines,
    ReceiverNonterminalObservationPhase, ReceiverObservation, ReceiverObservationSet,
    receiver_acceptance_expires_at,
};

impl Db {
    /// Commit post-spawn evidence only for the exact live pre-spawn owner.
    pub fn commit_receiver_job_launch(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        observation: &ReceiverLaunchObservation,
    ) -> Result<bool> {
        self.commit_receiver_job_launch_for_attempt(
            job_id,
            owner,
            observation,
            crate::state::ReceiverAttemptKind::Ordinary,
        )
    }

    /// Commit post-spawn evidence only for an exact live recovery owner.
    pub fn commit_receiver_recovery_job_launch(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        observation: &ReceiverLaunchObservation,
    ) -> Result<bool> {
        self.commit_receiver_job_launch_for_attempt(
            job_id,
            owner,
            observation,
            crate::state::ReceiverAttemptKind::Recovery,
        )
    }

    fn commit_receiver_job_launch_for_attempt(
        &self,
        job_id: ReceiverJobId,
        owner: &str,
        observation: &ReceiverLaunchObservation,
        expected_attempt: crate::state::ReceiverAttemptKind,
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
        let changed = self.conn.execute(
            "UPDATE receiver_jobs
             SET state = 'launched', launched_at_unix_ms = ?6,
                 observation_instance = ?4, observation_session_id = ?5,
                 observation_revision = 0, acceptance_expires_at_unix_ms = ?9,
                 updated_at_unix_ms = ?6
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND claim_owner = ?7 AND claim_expires_at_unix_ms > ?8
               AND state = 'launching' AND attempt_kind = ?10",
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
                match expected_attempt {
                    crate::state::ReceiverAttemptKind::Ordinary => "ordinary",
                    crate::state::ReceiverAttemptKind::Recovery => "recovery",
                },
            ],
        )? == 1;
        if changed {
            self.log_receiver_summary(|summary| {
                crate::logging::ReceiverLifecycleEvent::launch(
                    summary.recovery_attempt().unwrap_or(0),
                    summary.recovery_limit(),
                )
            });
        }
        Ok(changed)
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
            latest_progress_at_unix_ms: None,
            completed_at_unix_ms: None,
            authorized_at_unix_ms: observation.authorized_at_unix_ms,
        };
        match observation.phase {
            ReceiverNonterminalObservationPhase::Accepted => {
                set.accepted_at_unix_ms = Some(observation.observed_at_unix_ms);
            }
            ReceiverNonterminalObservationPhase::Progressing => {
                set.progressing_at_unix_ms = Some(observation.observed_at_unix_ms);
                set.latest_progress_at_unix_ms = Some(observation.observed_at_unix_ms);
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
        let latest_progress = observation
            .latest_progress_at_unix_ms
            .map(|value| to_i64(value, "receiver latest-progress observation time"))
            .transpose()?;
        let completed = observation
            .completed_at_unix_ms
            .map(|value| to_i64(value, "receiver completed observation time"))
            .transpose()?;
        anyhow::ensure!(
            accepted.is_some()
                || progressing.is_some()
                || latest_progress.is_some()
                || completed.is_some(),
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
                    .zip(latest_progress)
                    .is_none_or(|(first, latest)| first <= latest)
                && latest_progress
                    .zip(completed)
                    .is_none_or(|(latest, last)| latest <= last)
                && (progressing.is_none() || latest_progress.is_some()),
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
        } else if latest_progress.is_some() {
            ("processing", "'processing'")
        } else {
            ("accepted", "'launched'")
        };
        let exact_scope = format!(
            "workspace_id = ?1 AND job_id = ?2 AND job_token = ?3 AND claim_owner = ?4
                   AND claim_expires_at_unix_ms > ?5 AND observation_instance = ?6
                   AND (observation_revision = 0 OR observation_session_id = ?7)
                   AND EXISTS(
                     SELECT 1 FROM brain_sessions AS active
                     JOIN receiver_session_registrations AS registration
                       ON registration.workspace_id = active.workspace_id
                      AND registration.brain_instance_id = active.brain_instance_id
                      AND registration.agent_kind = active.agent_kind
                      AND registration.actor_id = active.actor_id
                      AND registration.channel = active.channel
                     WHERE active.workspace_id = ?1 AND active.brain_instance_id = ?6
                       AND active.agent_session_id = ?7 AND active.locked_pid IS NOT NULL
                       AND registration.conversation_id = receiver_jobs.conversation_id
                   )
                   AND observation_revision < ?8 AND state IN ({states})
                   AND (?9 IS NULL OR latest_progress_at_unix_ms IS NULL
                        OR latest_progress_at_unix_ms < ?9)"
        );
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let stored_deadlines = transaction
            .query_row(
                &format!(
                    "SELECT progress_expires_at_unix_ms,
                            absolute_work_expires_at_unix_ms,
                            latest_progress_at_unix_ms
                     FROM receiver_jobs WHERE {exact_scope}"
                ),
                rusqlite::params![
                    self.workspace_id,
                    job_id.to_string(),
                    observation.token.to_string(),
                    owner,
                    authorized,
                    instance,
                    session_id,
                    revision,
                    latest_progress,
                ],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((stored_progress_expires, stored_absolute_work_expires, stored_latest_progress)) =
            stored_deadlines
        else {
            return Ok(false);
        };
        let base_deadlines = if let Some(absolute) = stored_absolute_work_expires {
            ReceiverLifecycleDeadlines {
                progress_expires_at_unix_ms: u64::try_from(
                    stored_progress_expires.unwrap_or(absolute),
                )
                .map_err(|_| anyhow::anyhow!("receiver progress expiry is negative"))?,
                absolute_work_expires_at_unix_ms: u64::try_from(absolute)
                    .map_err(|_| anyhow::anyhow!("receiver absolute-work expiry is negative"))?,
                latest_progress_at_unix_ms: stored_latest_progress
                    .map(|value| {
                        u64::try_from(value).map_err(|_| {
                            anyhow::anyhow!("receiver latest-progress evidence is negative")
                        })
                    })
                    .transpose()?,
            }
        } else {
            let accepted_at_unix_ms = observation
                .accepted_at_unix_ms
                .ok_or_else(|| anyhow::anyhow!("receiver acceptance deadline is missing"))?;
            ReceiverLifecycleDeadlines::after_acceptance(
                observation.authorized_at_unix_ms,
                accepted_at_unix_ms,
            )
        };
        let deadlines = observation
            .latest_progress_at_unix_ms
            .map_or(base_deadlines, |latest| {
                base_deadlines.after_progress(observation.authorized_at_unix_ms, latest)
            });
        let progress_expires = to_i64(
            deadlines.progress_expires_at_unix_ms,
            "receiver progress expiry",
        )?;
        let absolute_work_expires = to_i64(
            deadlines.absolute_work_expires_at_unix_ms,
            "receiver absolute-work expiry",
        )?;
        let sql = format!(
            "UPDATE receiver_jobs SET state = ?10,
                 accepted_at_unix_ms = COALESCE(accepted_at_unix_ms, ?11),
                 progressing_at_unix_ms = COALESCE(progressing_at_unix_ms, ?12),
                 attempt_accepted_at_unix_ms = COALESCE(attempt_accepted_at_unix_ms, ?11),
                 attempt_progressing_at_unix_ms = COALESCE(attempt_progressing_at_unix_ms, ?12),
                 latest_progress_at_unix_ms = COALESCE(?9, latest_progress_at_unix_ms),
                 progress_expires_at_unix_ms = ?13,
                 absolute_work_expires_at_unix_ms = COALESCE(
                   absolute_work_expires_at_unix_ms, ?14
                 ),
                 observation_revision = ?8, observation_session_id = ?7,
                 updated_at_unix_ms = ?5
                 WHERE {exact_scope}"
        );
        let changed = transaction.execute(
            &sql,
            rusqlite::params![
                self.workspace_id,
                job_id.to_string(),
                observation.token.to_string(),
                owner,
                authorized,
                instance,
                session_id,
                revision,
                latest_progress,
                next,
                accepted,
                progressing,
                progress_expires,
                absolute_work_expires,
            ],
        )?;
        if changed == 1 {
            transaction.commit()?;
            crate::logging::log_receiver_lifecycle(
                crate::logging::ReceiverLifecycleEvent::observation(if next == "accepted" {
                    crate::logging::ReceiverLifecyclePhase::Accepted
                } else {
                    crate::logging::ReceiverLifecyclePhase::Processing
                }),
            );
        }
        Ok(changed == 1)
    }
}
