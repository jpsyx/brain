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
use super::diagnostic::receiver_observation_diagnostic;
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
                self.stop_locally_after_lost_receiver_ownership(&active, None, "ownership-changed");
                return;
            }
            Err(_) => {
                self.log_receiver_observation(&active, None, "claim-renewal-store");
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
            self.stop_locally_after_lost_receiver_ownership(&active, None, "tab-identity-mismatch");
            return;
        };
        let poll = self.poll_active_receiver_run(&active);
        let path = self.receiver_completion_path(active.attribution.instance());
        if let Some(completion) = self.exact_receiver_completion(&active, &path) {
            #[cfg(test)]
            self.receiver.run_after_completion_validation_hook();
            let completion_observed_at = self.receiver_now_unix_ms();
            let boundary = poll.as_ref().ok().and_then(|(poll, _)| {
                poll.observation
                    .boundaries()
                    .last()
                    .map(|boundary| boundary.phase())
            });
            self.log_receiver_observation(&active, boundary, "artifact-precedence");
            self.finish_completed_receiver_run(
                active,
                &completion.session,
                &completion.message,
                completion_observed_at,
            );
            return;
        }

        match poll {
            Ok((poll, _)) if poll.observation.boundaries().is_empty() => {
                if poll.exited {
                    self.log_receiver_observation(&active, None, "child-exit");
                    self.clean_exited_receiver_run_locally(&active);
                } else {
                    self.log_receiver_observation(&active, None, "pending");
                    self.receiver.store_durable_run(
                        crate::tui::receiver::DurableReceiverRun::Active(active),
                    );
                }
            }
            Ok((poll, prior_state)) => {
                self.apply_active_receiver_observation(active, &poll, prior_state);
            }
            Err(ReceiverRunPollError::MissingTab) => {
                self.stop_locally_after_lost_receiver_ownership(&active, None, "tab-missing");
            }
            Err(ReceiverRunPollError::IdentityMismatch) => {
                self.stop_locally_after_lost_receiver_ownership(
                    &active,
                    None,
                    "tab-identity-mismatch",
                );
            }
            Err(ReceiverRunPollError::Observation(error)) => {
                if tab.exited {
                    self.log_receiver_observation(&active, None, "child-exit");
                    self.clean_exited_receiver_run_locally(&active);
                    return;
                }
                self.log_receiver_observation(&active, None, observation_error_category(error));
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
            &active.attribution,
            &poll.observation,
            authorized_at_unix_ms,
        ) {
            Ok(outcome) if outcome.changed && outcome.completed => {
                crate::logging::log(receiver_observation_diagnostic(
                    active.claim.job().id(),
                    active.attribution.instance(),
                    active.attribution.scope().agent_kind(),
                    prior_state,
                    Some(boundary),
                    "persisted-terminal",
                ));
                self.finish_observation_only_receiver_run(&active);
            }
            Ok(outcome) if outcome.changed => {
                crate::logging::log(receiver_observation_diagnostic(
                    active.claim.job().id(),
                    active.attribution.instance(),
                    active.attribution.scope().agent_kind(),
                    prior_state,
                    Some(boundary),
                    "persisted",
                ));
                self.receiver
                    .store_durable_run(crate::tui::receiver::DurableReceiverRun::Active(active));
            }
            Ok(_) => {
                crate::logging::log(receiver_observation_diagnostic(
                    active.claim.job().id(),
                    active.attribution.instance(),
                    active.attribution.scope().agent_kind(),
                    prior_state,
                    Some(boundary),
                    "not-committed",
                ));
                let now = self.receiver_now_unix_ms();
                match self.services.renew_receiver_claim(
                    active.claim.job().id(),
                    active.claim.claim().owner(),
                    now,
                    now.saturating_add(CLAIM_LIFETIME_MS),
                ) {
                    Ok(false) => self.stop_locally_after_lost_receiver_ownership(
                        &active,
                        Some(boundary),
                        "ownership-changed",
                    ),
                    Ok(true) | Err(_) => self.receiver.store_durable_run(
                        crate::tui::receiver::DurableReceiverRun::Active(active),
                    ),
                }
            }
            Err(_) => {
                crate::logging::log(receiver_observation_diagnostic(
                    active.claim.job().id(),
                    active.attribution.instance(),
                    active.attribution.scope().agent_kind(),
                    prior_state,
                    Some(boundary),
                    "store",
                ));
                self.receiver
                    .store_durable_run(crate::tui::receiver::DurableReceiverRun::Active(active));
            }
        }
    }

    fn finish_observation_only_receiver_run(&mut self, active: &ActiveReceiverRun) {
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
            self.log_receiver_observation(
                active,
                Some(AgentObservationPhase::Completed),
                "session-release-store",
            );
        }
        self.remove_exact_receiver_tab(active);
        self.cleanup_receiver_instance_files(active.attribution.instance());
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
                self.log_receiver_observation(&active, None, "artifact-not-committed");
                self.receiver
                    .store_durable_run(crate::tui::receiver::DurableReceiverRun::Active(active));
                return;
            }
            Err(_) => {
                self.log_receiver_observation(&active, None, "artifact-store");
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
        if self
            .services
            .release_receiver_session(&active.attribution)
            .is_err()
        {
            self.log_receiver_observation(
                &active,
                Some(AgentObservationPhase::Completed),
                "session-release-store",
            );
        }
        self.remove_exact_receiver_tab(&active);
        self.cleanup_receiver_instance_files(active.attribution.instance());
        crate::logging::log(format!(
            "receiver run completed channel={:?}",
            active.claim.job().inbound().channel
        ));
        self.reload_after_brain();
    }

    fn clean_exited_receiver_run_locally(&mut self, active: &ActiveReceiverRun) {
        self.remove_exact_receiver_tab(active);
        self.cleanup_receiver_instance_files(active.attribution.instance());
        crate::logging::log("receiver exited after launch; durable evidence remains unchanged");
    }

    fn stop_locally_after_lost_receiver_ownership(
        &mut self,
        active: &ActiveReceiverRun,
        boundary: Option<AgentObservationPhase>,
        category: &'static str,
    ) {
        self.log_receiver_observation(active, boundary, category);
        self.remove_exact_receiver_tab(active);
        self.cleanup_receiver_instance_files(active.attribution.instance());
    }

    fn remove_exact_receiver_tab(&mut self, active: &ActiveReceiverRun) {
        let removed = self.brain.remove_receiver_run(active.tab_id);
        if removed.as_ref().is_some_and(|removed| {
            removed.job_id != active.claim.job().id()
                || removed.instance != active.attribution.instance()
        }) {
            self.log_receiver_observation(active, None, "tab-cleanup-identity-mismatch");
        }
    }

    fn log_receiver_observation(
        &self,
        active: &ActiveReceiverRun,
        boundary: Option<AgentObservationPhase>,
        category: &'static str,
    ) {
        let prior = self
            .services
            .receiver_observation_cursor(active.claim.job().id())
            .ok()
            .flatten()
            .map_or(ReceiverJobState::Launched, |(state, _)| state);
        crate::logging::log(receiver_observation_diagnostic(
            active.claim.job().id(),
            active.attribution.instance(),
            active.attribution.scope().agent_kind(),
            prior,
            boundary,
            category,
        ));
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
