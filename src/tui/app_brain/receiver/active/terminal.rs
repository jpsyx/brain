//! Exact terminal authorization, effects, and local cleanup.

use crate::agent::{AgentObservationPhase, AgentSession, CompletionStatus, SessionStore};
use crate::tui::App;
use crate::tui::receiver::{ActiveReceiverRun, CleanupPendingReceiverRun};

use super::super::artifact::{CompletionExpectation, ReceiverCompletion, read_exact_completion};

impl App {
    pub(super) fn finish_observation_only_receiver_run(&mut self, active: &ActiveReceiverRun) {
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

    pub(super) fn exact_receiver_completion(
        &self,
        active: &ActiveReceiverRun,
        path: &std::path::Path,
    ) -> Option<ReceiverCompletion> {
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

    pub(super) fn finish_completed_receiver_run(
        &mut self,
        active: ActiveReceiverRun,
        completed_session: &AgentSession,
        message: &str,
        observation: Option<&crate::state::ReceiverObservationSet>,
        observed_at_unix_ms: u64,
        authorized_at_unix_ms: u64,
    ) {
        let request = crate::state::ReceiverCompletionRequest {
            job_id: active.claim.job().id(),
            token: active.claim.job().token(),
            owner: active.claim.claim().owner(),
            registration: &active.attribution,
            completed_session,
            answer: message,
            observed_at_unix_ms,
            authorized_at_unix_ms,
        };
        let completed = self
            .services
            .complete_receiver_job_with_observation(&request, observation);
        match completed {
            Ok(Some(_)) => {}
            Ok(None) => {
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
        #[cfg(test)]
        self.receiver.run_after_completion_commit_hook();
        crate::logging::log(format!(
            "receiver run completed channel={:?}",
            active.claim.job().inbound().channel
        ));
        self.begin_receiver_answer_cleanup(active);
    }

    pub(super) fn clean_exited_receiver_run_locally(&mut self, active: &ActiveReceiverRun) {
        if active.claim.job().attempt_kind() == crate::state::ReceiverAttemptKind::Recovery {
            let now = self.receiver_now_unix_ms();
            let pid = i32::try_from(std::process::id()).unwrap_or(0);
            match self.services.establish_receiver_spawned_recovery_cleanup(
                active.claim.job().id(),
                active.claim.job().token(),
                active.claim.claim().owner(),
                &active.attribution,
                pid,
                now,
            ) {
                Ok(crate::state::ReceiverRecoveryCleanupOutcome::Exact(effect)) => {
                    self.continue_receiver_cleanup(CleanupPendingReceiverRun {
                        active: ActiveReceiverRun {
                            claim: active.claim.clone(),
                            attribution: active.attribution.clone(),
                            tab_id: active.tab_id,
                            _attachments: crate::tui::receiver::attachments::PreparedReceiverAttachments::empty(),
                        },
                        effect,
                        shutdown_complete: false,
                        artifacts_removed: false,
                        defer_once: false,
                    });
                    return;
                }
                Ok(crate::state::ReceiverRecoveryCleanupOutcome::Changed) => {
                    self.preserve_recovery_active(active);
                    return;
                }
                Err(_) => {
                    crate::logging::log(format!(
                        "receiver recovery failed job={} boundary=process-exit-store",
                        active.claim.job().id()
                    ));
                    self.preserve_recovery_active(active);
                    return;
                }
            }
        }
        self.remove_exact_receiver_tab(active);
        self.cleanup_receiver_instance_files(active.attribution.instance());
        crate::logging::log("receiver exited after launch; durable evidence remains unchanged");
    }

    fn preserve_recovery_active(&mut self, active: &ActiveReceiverRun) {
        self.receiver
            .store_durable_run(crate::tui::receiver::DurableReceiverRun::Active(
                crate::tui::receiver::ActiveReceiverRun {
                    claim: active.claim.clone(),
                    attribution: active.attribution.clone(),
                    tab_id: active.tab_id,
                    _attachments:
                        crate::tui::receiver::attachments::PreparedReceiverAttachments::empty(),
                },
            ));
    }

    pub(super) fn stop_locally_after_lost_receiver_ownership(
        &mut self,
        active: ActiveReceiverRun,
        boundary: Option<AgentObservationPhase>,
        category: &'static str,
    ) {
        self.log_receiver_observation(&active, boundary, category);
        if active.claim.job().attempt_kind() == crate::state::ReceiverAttemptKind::Recovery {
            let now = self.receiver_now_unix_ms();
            let pid = i32::try_from(std::process::id()).unwrap_or(0);
            match self.services.establish_receiver_spawned_recovery_cleanup(
                active.claim.job().id(),
                active.claim.job().token(),
                active.claim.claim().owner(),
                &active.attribution,
                pid,
                now,
            ) {
                Ok(crate::state::ReceiverRecoveryCleanupOutcome::Exact(effect)) => {
                    self.continue_receiver_cleanup(CleanupPendingReceiverRun {
                        active,
                        effect,
                        shutdown_complete: false,
                        artifacts_removed: false,
                        defer_once: false,
                    });
                }
                Ok(crate::state::ReceiverRecoveryCleanupOutcome::Changed) | Err(_) => {
                    self.receiver.store_durable_run(
                        crate::tui::receiver::DurableReceiverRun::Active(active),
                    );
                }
            }
            return;
        }
        self.remove_exact_receiver_tab(&active);
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
}
