//! Renewal and terminal handling for one launched receiver process.

use crate::agent::{
    AgentObservationError, AgentObservationPhase, AgentObservationRequest, AgentSession,
    CompletionStatus, SessionStore,
};
use crate::state::ReceiverJobState;
use crate::tui::App;
use crate::tui::receiver::ActiveReceiverRun;
use crate::tui::state::ReceiverRunPollError;

use super::artifact::{CompletionExpectation, read_exact_completion};
use super::dispatch::CLAIM_LIFETIME_MS;

impl App {
    pub(super) fn tick_active_receiver_run(&mut self, active: ActiveReceiverRun) {
        let now = self.receiver_now_unix_ms();
        match self.services.renew_receiver_claim(
            active.claim.job().id(),
            active.claim.claim().owner(),
            now,
            now.saturating_add(CLAIM_LIFETIME_MS),
        ) {
            Ok(true) => {}
            Ok(false) => {
                self.stop_locally_after_lost_receiver_ownership(&active);
                return;
            }
            Err(error) => {
                crate::logging::log(format!("receiver claim renewal failed: {error:#}"));
                self.receiver
                    .store_durable_run(crate::tui::receiver::DurableReceiverRun::Active(active));
                return;
            }
        }

        let tab = self
            .brain
            .receiver_run_observations()
            .into_iter()
            .find(|observation| {
                observation.id == active.tab_id
                    && observation.job_id == active.claim.job().id()
                    && observation.instance == active.attribution.instance()
            });
        let Some(tab) = tab else {
            self.stop_locally_after_lost_receiver_ownership(&active);
            return;
        };
        let poll = self.poll_active_receiver_run(&active);
        let path = self.receiver_completion_path(active.attribution.instance());
        if let Some(completion) = self.exact_receiver_completion(&active, &path) {
            #[cfg(test)]
            self.receiver.run_after_completion_validation_hook();
            let completion_observed_at = self.receiver_now_unix_ms();
            self.finish_completed_receiver_run(
                active,
                &completion.session,
                &completion.message,
                &path,
                completion_observed_at,
            );
            return;
        }

        match poll {
            Ok((poll, _)) if poll.observation.boundaries().is_empty() => {
                if poll.exited {
                    self.clean_exited_receiver_run_locally(&active, &path);
                } else {
                    self.receiver.store_durable_run(
                        crate::tui::receiver::DurableReceiverRun::Active(active),
                    );
                }
            }
            Ok((poll, prior_state)) => {
                self.apply_active_receiver_observation(active, &poll, prior_state, &path);
            }
            Err(ReceiverRunPollError::MissingTab | ReceiverRunPollError::IdentityMismatch) => {
                self.stop_locally_after_lost_receiver_ownership(&active);
            }
            Err(ReceiverRunPollError::Observation(error)) => {
                if tab.exited {
                    self.clean_exited_receiver_run_locally(&active, &path);
                    return;
                }
                crate::logging::log(format!(
                    "receiver observation rejected job={} instance={} frontend={} category={}",
                    active.claim.job().id(),
                    active.attribution.instance(),
                    active.attribution.scope().agent_kind().as_str(),
                    observation_error_category(error),
                ));
                self.receiver
                    .store_durable_run(crate::tui::receiver::DurableReceiverRun::Active(active));
            }
        }
    }

    fn poll_active_receiver_run(
        &self,
        active: &ActiveReceiverRun,
    ) -> Result<(crate::tui::state::ReceiverRunPoll, ReceiverJobState), ReceiverRunPollError> {
        let (prior_state, cursor) = self
            .services
            .receiver_observation_cursor(active.claim.job().id())
            .map_err(|_| ReceiverRunPollError::IdentityMismatch)?
            .ok_or(ReceiverRunPollError::IdentityMismatch)?;
        let session = self
            .services
            .locked_session_for_instance(active.attribution.instance(), active.attribution.scope())
            .and_then(|session| AgentSession::new(session).ok())
            .ok_or(ReceiverRunPollError::Observation(
                AgentObservationError::OwnershipUnavailable,
            ))?;
        let request = AgentObservationRequest::new(
            active.claim.job().token().to_string(),
            active.attribution.instance(),
            self.receiver_observation_path(active.attribution.instance()),
            session,
            cursor,
        );
        let poll = self.brain.poll_receiver_run(
            active.tab_id,
            active.claim.job().id(),
            active.attribution.instance(),
            &request,
        )?;
        Ok((poll, prior_state))
    }

    fn apply_active_receiver_observation(
        &mut self,
        active: ActiveReceiverRun,
        poll: &crate::tui::state::ReceiverRunPoll,
        prior_state: ReceiverJobState,
        path: &std::path::Path,
    ) {
        let boundary = poll
            .observation
            .boundaries()
            .last()
            .map_or(AgentObservationPhase::Launched, |boundary| boundary.phase());
        #[cfg(test)]
        self.receiver.run_after_observation_validation_hook();
        let authorized_at_unix_ms = self.receiver_now_unix_ms();
        match self.services.apply_receiver_observation_result(
            active.claim.job().id(),
            active.claim.job().token(),
            active.claim.claim().owner(),
            active.attribution.instance(),
            &poll.observation,
            authorized_at_unix_ms,
        ) {
            Ok(outcome) if outcome.changed && outcome.completed => {
                self.finish_observation_only_receiver_run(&active, path);
            }
            Ok(outcome) if outcome.changed => {
                crate::logging::log(format!(
                    "receiver observation persisted job={} instance={} frontend={} prior={:?} boundary={boundary:?}",
                    active.claim.job().id(),
                    active.attribution.instance(),
                    active.attribution.scope().agent_kind().as_str(),
                    prior_state,
                ));
                self.receiver
                    .store_durable_run(crate::tui::receiver::DurableReceiverRun::Active(active));
            }
            Ok(_) => {
                let now = self.receiver_now_unix_ms();
                match self.services.renew_receiver_claim(
                    active.claim.job().id(),
                    active.claim.claim().owner(),
                    now,
                    now.saturating_add(CLAIM_LIFETIME_MS),
                ) {
                    Ok(false) => self.stop_locally_after_lost_receiver_ownership(&active),
                    Ok(true) | Err(_) => self.receiver.store_durable_run(
                        crate::tui::receiver::DurableReceiverRun::Active(active),
                    ),
                }
            }
            Err(_) => {
                crate::logging::log(format!(
                    "receiver observation commit failed job={} instance={} frontend={} category=store",
                    active.claim.job().id(),
                    active.attribution.instance(),
                    active.attribution.scope().agent_kind().as_str(),
                ));
                self.receiver
                    .store_durable_run(crate::tui::receiver::DurableReceiverRun::Active(active));
            }
        }
    }

    fn finish_observation_only_receiver_run(
        &mut self,
        active: &ActiveReceiverRun,
        path: &std::path::Path,
    ) {
        if crate::sync::config::SyncConfig::load(self.context.command()).is_configured() {
            let _ = self
                .services
                .spawn_detached_sync(self.context.workspace(), crate::sync::args::Direction::Push);
        }
        if self
            .services
            .release_receiver_session(&active.attribution)
            .is_err()
        {
            crate::logging::log("receiver session release failed category=store");
        }
        self.remove_exact_receiver_tab(active);
        let _ = std::fs::remove_file(path);
        crate::logging::log(format!(
            "receiver run completed from lifecycle observation job={} instance={} frontend={}",
            active.claim.job().id(),
            active.attribution.instance(),
            active.attribution.scope().agent_kind().as_str(),
        ));
        self.reload_after_brain();
    }

    fn exact_receiver_completion(
        &self,
        active: &ActiveReceiverRun,
        path: &std::path::Path,
    ) -> Option<super::artifact::ReceiverCompletion> {
        let attribution = &active.attribution;
        let actual_session = self
            .services
            .locked_session_for_instance(attribution.instance(), attribution.scope())?;
        let actual_session = AgentSession::new(actual_session).ok()?;
        if SessionStore::completion_status(&self.services, &actual_session, attribution.scope())
            != Some(CompletionStatus::Completed)
        {
            return None;
        }
        let workspace_id = attribution.scope().workspace_id().to_string();
        let job_token = active.claim.job().token().to_string();
        read_exact_completion(
            path,
            &CompletionExpectation {
                job_token: &job_token,
                session_id: actual_session.as_str(),
                response_id: attribution.instance(),
                frontend: attribution.scope().agent_kind().as_str(),
                workspace_id: &workspace_id,
                actor_id: attribution.scope().actor().user_id().as_str(),
                channel: attribution.scope().actor().channel().as_str(),
            },
        )
    }

    fn finish_completed_receiver_run(
        &mut self,
        active: ActiveReceiverRun,
        completed_session: &AgentSession,
        message: &str,
        path: &std::path::Path,
        now: u64,
    ) {
        let completed = self.services.complete_receiver_job_with_binding(
            active.claim.job().id(),
            active.claim.job().token(),
            active.claim.claim().owner(),
            &active.attribution,
            completed_session,
            now,
        );
        match completed {
            Ok(true) => {}
            Ok(false) => {
                self.receiver
                    .store_durable_run(crate::tui::receiver::DurableReceiverRun::Active(active));
                return;
            }
            Err(error) => {
                crate::logging::log(format!("receiver completion commit failed: {error:#}"));
                self.receiver
                    .store_durable_run(crate::tui::receiver::DurableReceiverRun::Active(active));
                return;
            }
        }
        if crate::sync::config::SyncConfig::load(self.context.command()).is_configured() {
            let _ = self
                .services
                .spawn_detached_sync(self.context.workspace(), crate::sync::args::Direction::Push);
        }
        self.reply_to_job(
            active.claim.job().inbound(),
            "final receiver response",
            message,
        );
        if let Err(error) = self.services.release_receiver_session(&active.attribution) {
            crate::logging::log(format!("receiver session release failed: {error:#}"));
        }
        self.remove_exact_receiver_tab(&active);
        let _ = std::fs::remove_file(path);
        crate::logging::log(format!(
            "receiver run completed channel={:?}",
            active.claim.job().inbound().channel
        ));
        self.reload_after_brain();
    }

    fn clean_exited_receiver_run_locally(
        &mut self,
        active: &ActiveReceiverRun,
        path: &std::path::Path,
    ) {
        self.remove_exact_receiver_tab(active);
        let _ = std::fs::remove_file(path);
        crate::logging::log("receiver exited after launch; durable evidence remains unchanged");
    }

    fn stop_locally_after_lost_receiver_ownership(&mut self, active: &ActiveReceiverRun) {
        self.remove_exact_receiver_tab(active);
        let _ = std::fs::remove_file(self.receiver_completion_path(active.attribution.instance()));
        crate::logging::log("receiver run stopped after durable claim ownership changed");
    }

    fn remove_exact_receiver_tab(&mut self, active: &ActiveReceiverRun) {
        let removed = self.brain.remove_receiver_run(active.tab_id);
        if removed.as_ref().is_some_and(|removed| {
            removed.job_id != active.claim.job().id()
                || removed.instance != active.attribution.instance()
        }) {
            crate::logging::log("receiver tab identity changed before exact cleanup");
        }
    }

    fn receiver_completion_path(&self, instance: &str) -> std::path::PathBuf {
        self.context
            .workspace()
            .paths()
            .responses_dir()
            .join(format!("{instance}.json"))
    }
}

const fn observation_error_category(error: AgentObservationError) -> &'static str {
    match error {
        AgentObservationError::InvalidIdentifier => "invalid-identifier",
        AgentObservationError::WrongPath => "wrong-path",
        AgentObservationError::PlaceholderSession => "placeholder-session",
        AgentObservationError::OwnershipUnavailable => "ownership-unavailable",
        AgentObservationError::SessionOwnership => "session-ownership",
        AgentObservationError::InvalidFileType => "invalid-file-type",
        AgentObservationError::InvalidPermissions => "invalid-permissions",
        AgentObservationError::SnapshotTooLarge => "snapshot-too-large",
        AgentObservationError::TruncatedSnapshot => "truncated-snapshot",
        AgentObservationError::MalformedSnapshot => "malformed-snapshot",
        AgentObservationError::IdentityMismatch => "identity-mismatch",
        AgentObservationError::SessionMismatch => "session-mismatch",
        AgentObservationError::RevisionRegression => "revision-regression",
        AgentObservationError::AmbiguousLifecycle => "ambiguous-lifecycle",
    }
}
