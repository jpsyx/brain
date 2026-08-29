//! Renewal and terminal handling for one launched receiver process.

use crate::agent::{
    AgentObservationError, AgentObservationPhase, AgentObservationRequest, AgentSession,
};
use crate::state::ReceiverJobState;
use crate::tui::App;
use crate::tui::receiver::ActiveReceiverRun;
use crate::tui::state::{AppServices, ReceiverRunPollError};

use super::diagnostic::receiver_observation_diagnostic;
use super::dispatch::CLAIM_LIFETIME_MS;

mod terminal;

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
                self.stop_locally_after_lost_receiver_ownership(active, None, "ownership-changed");
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
            self.stop_locally_after_lost_receiver_ownership(active, None, "tab-identity-mismatch");
            return;
        };
        let poll = self.poll_active_receiver_run(&active);
        let path = self.receiver_completion_path(active.attribution.instance());
        if let Some(completion) = self.exact_receiver_completion(&active, &path) {
            #[cfg(test)]
            self.receiver.run_after_completion_validation_hook();
            let producer_completed_at = poll
                .as_ref()
                .ok()
                .and_then(|(poll, _)| {
                    poll.observation
                        .boundaries()
                        .iter()
                        .find(|boundary| boundary.phase() == AgentObservationPhase::Completed)
                })
                .map(|boundary| boundary.observed_at_unix_ms());
            let completion_authorized_at = self.receiver_now_unix_ms();
            let completion_observed_at = producer_completed_at.unwrap_or(completion_authorized_at);
            let completion_observation = poll
                .as_ref()
                .ok()
                .map(|(poll, _)| &poll.observation)
                .filter(|observation| observation.has_updates())
                .map(|observation| {
                    AppServices::receiver_observation_set(
                        active.claim.job().token(),
                        &active.attribution,
                        observation,
                        completion_authorized_at,
                    )
                });
            let boundary = poll.as_ref().ok().and_then(|(poll, _)| {
                poll.observation.boundaries().last().map_or_else(
                    || {
                        poll.observation
                            .progress_pulse()
                            .map(|_| AgentObservationPhase::Progressing)
                    },
                    |boundary| Some(boundary.phase()),
                )
            });
            self.log_receiver_observation(&active, boundary, "artifact-precedence");
            self.finish_completed_receiver_run(
                active,
                &completion.session,
                &completion.message,
                completion_observation.as_ref(),
                completion_observed_at,
                completion_authorized_at,
            );
            return;
        }

        match poll {
            Ok((poll, _)) if !poll.observation.has_updates() => {
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
                self.stop_locally_after_lost_receiver_ownership(active, None, "tab-missing");
            }
            Err(ReceiverRunPollError::IdentityMismatch) => {
                self.stop_locally_after_lost_receiver_ownership(
                    active,
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
        let boundary = poll.observation.boundaries().last().map_or_else(
            || {
                if poll.observation.progress_pulse().is_some() {
                    AgentObservationPhase::Progressing
                } else {
                    AgentObservationPhase::Launched
                }
            },
            |boundary| boundary.phase(),
        );
        #[cfg(test)]
        self.receiver
            .run_before_observation_persistence_hook(poll.observation.boundaries());
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
            Ok(true) => {
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
            Ok(false) => {
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
                        active,
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
        let diagnostic = receiver_observation_diagnostic(
            active.claim.job().id(),
            active.attribution.instance(),
            active.attribution.scope().agent_kind(),
            prior,
            boundary,
            category,
        );
        #[cfg(test)]
        self.receiver
            .record_observation_diagnostic(diagnostic.clone());
        crate::logging::log(diagnostic);
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
