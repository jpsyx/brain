use anyhow::Result;

use super::{to_i64, validated_owner};
use crate::state::{
    Db, ReceiverJobId, ReceiverLaunchObservation, ReceiverObservation, ReceiverObservationPhase,
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
        Ok(self.conn.execute(
            "UPDATE receiver_jobs
             SET state = 'launched', launched_at_unix_ms = ?6,
                 observation_instance = ?4, observation_session_id = ?5,
                 observation_revision = 0, updated_at_unix_ms = ?6
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
        let owner = validated_owner(owner)?;
        let instance = validated_owner(&observation.instance)?;
        let session_id = validated_owner(&observation.session_id)?;
        let revision = to_i64(observation.revision, "receiver observation revision")?;
        let observed = to_i64(observation.observed_at_unix_ms, "receiver observation time")?;
        let authorized = to_i64(
            observation.authorized_at_unix_ms,
            "receiver observation authorization time",
        )?;
        let (next, states) = match observation.phase {
            ReceiverObservationPhase::Accepted => ("accepted", "'launched'"),
            ReceiverObservationPhase::Progressing => ("processing", "'launched', 'accepted'"),
            ReceiverObservationPhase::Completed => ("done", "'launched', 'accepted', 'processing'"),
        };
        let terminal = matches!(observation.phase, ReceiverObservationPhase::Completed);
        let sql = if terminal {
            format!(
                "UPDATE receiver_jobs SET state = ?8, completed_at_unix_ms = ?7,
                 observation_revision = ?6, claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL, updated_at_unix_ms = ?7
                 WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3 AND claim_owner = ?4
                   AND claim_expires_at_unix_ms > ?10 AND observation_instance = ?5
                   AND observation_session_id = ?9 AND observation_revision < ?6 AND state IN ({states})"
            )
        } else {
            format!(
                "UPDATE receiver_jobs SET state = ?8,
                 accepted_at_unix_ms = COALESCE(accepted_at_unix_ms, ?10),
                 progressing_at_unix_ms = COALESCE(progressing_at_unix_ms, ?11),
                 observation_revision = ?6, updated_at_unix_ms = ?7
                 WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3 AND claim_owner = ?4
                   AND claim_expires_at_unix_ms > ?12 AND observation_instance = ?5
                   AND observation_session_id = ?9 AND observation_revision < ?6 AND state IN ({states})"
            )
        };
        let changed = match observation.phase {
            ReceiverObservationPhase::Accepted => self.conn.execute(
                &sql,
                rusqlite::params![
                    self.workspace_id,
                    job_id.to_string(),
                    observation.token.to_string(),
                    owner,
                    instance,
                    revision,
                    observed,
                    next,
                    session_id,
                    observed,
                    Option::<i64>::None,
                    authorized,
                ],
            )?,
            ReceiverObservationPhase::Progressing => self.conn.execute(
                &sql,
                rusqlite::params![
                    self.workspace_id,
                    job_id.to_string(),
                    observation.token.to_string(),
                    owner,
                    instance,
                    revision,
                    observed,
                    next,
                    session_id,
                    Option::<i64>::None,
                    observed,
                    authorized,
                ],
            )?,
            ReceiverObservationPhase::Completed => self.conn.execute(
                &sql,
                rusqlite::params![
                    self.workspace_id,
                    job_id.to_string(),
                    observation.token.to_string(),
                    owner,
                    instance,
                    revision,
                    observed,
                    next,
                    session_id,
                    authorized,
                ],
            )?,
        };
        Ok(changed == 1)
    }
}
