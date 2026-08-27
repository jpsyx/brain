//! Durable receiver recovery facade.

use anyhow::Result;

use super::AppServices;

impl AppServices {
    pub(crate) fn claim_receiver_recovery_run(
        &self,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Option<crate::state::ReceiverRunClaim>> {
        self.db
            .claim_next_receiver_recovery_run(owner, now_unix_ms, expires_at_unix_ms)
    }

    pub(crate) fn reconcile_next_receiver_job(
        &self,
        now_unix_ms: u64,
    ) -> Result<Option<crate::state::ReceiverReconciliationEffect>> {
        self.db.reconcile_next_receiver_job(now_unix_ms)
    }

    pub(crate) fn acknowledge_receiver_recovery_cleanup(
        &self,
        effect: &crate::state::ReceiverReconciliationEffect,
        now_unix_ms: u64,
    ) -> Result<bool> {
        let (Some(instance), Some(session_id)) =
            (effect.cleanup_instance(), effect.cleanup_session_id())
        else {
            return Ok(false);
        };
        self.db.acknowledge_receiver_recovery_cleanup(
            effect.job_id(),
            effect.token(),
            instance,
            session_id,
            now_unix_ms,
        )
    }

    pub(crate) fn receiver_cleanup_registration_is_stale(
        &self,
        effect: &crate::state::ReceiverReconciliationEffect,
    ) -> Result<bool> {
        self.db.receiver_cleanup_registration_is_stale(effect)
    }

    pub(crate) fn fail_receiver_recovery_resume(
        &self,
        job_id: crate::state::ReceiverJobId,
        owner: &str,
        now_unix_ms: u64,
    ) -> Result<Option<crate::state::ReceiverReconciliationEffect>> {
        self.db
            .fail_receiver_recovery_resume(job_id, owner, now_unix_ms)
    }

    pub(crate) fn fail_receiver_recovery_attempt(
        &self,
        job_id: crate::state::ReceiverJobId,
        owner: &str,
        now_unix_ms: u64,
        failure: crate::state::ReceiverRecoveryFailure,
    ) -> Result<Option<crate::state::ReceiverReconciliationEffect>> {
        self.db
            .fail_receiver_recovery_attempt(job_id, owner, now_unix_ms, failure)
    }

    pub(crate) fn prepare_receiver_recovery_launch(
        &self,
        job_id: crate::state::ReceiverJobId,
        owner: &str,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        self.db
            .prepare_receiver_recovery_job_launch(job_id, owner, observed_at_unix_ms)
    }

    pub(crate) fn commit_receiver_recovery_job_launch(
        &self,
        job_id: crate::state::ReceiverJobId,
        owner: &str,
        observation: &crate::state::ReceiverLaunchObservation,
    ) -> Result<bool> {
        let committed = self
            .db
            .commit_receiver_recovery_job_launch(job_id, owner, observation)?;
        #[cfg(test)]
        if committed && self.receiver_recovery_commit_visible_error.replace(false) {
            anyhow::bail!("injected visible recovery launch-commit error");
        }
        Ok(committed)
    }

    pub(crate) fn receiver_recovery_launch_is_exact(
        &self,
        job_id: crate::state::ReceiverJobId,
        token: crate::state::ReceiverJobToken,
        attribution: &crate::state::ReceiverSessionAttribution,
    ) -> Result<bool> {
        Ok(self.db.receiver_job(job_id)?.is_some_and(|job| {
            job.token() == token
                && job.state() == crate::state::ReceiverJobState::Launched
                && job.attempt_kind() == crate::state::ReceiverAttemptKind::Recovery
                && job.observation_instance() == Some(attribution.instance())
                && job.observation_session_id() == Some(attribution.registered_session().as_str())
        }))
    }

    #[cfg(test)]
    pub(crate) fn inject_receiver_recovery_commit_visible_error(&self) {
        self.receiver_recovery_commit_visible_error.set(true);
    }
}
