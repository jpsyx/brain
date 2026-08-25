//! Renewal and terminal handling for one launched receiver process.

use crate::agent::{AgentSession, CompletionStatus, SessionStore};
use crate::state::ReceiverLaunchFailure;
use crate::tui::App;
use crate::tui::receiver::ActiveReceiverRun;

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

        let observation = self
            .brain
            .receiver_run_observations()
            .into_iter()
            .find(|observation| {
                observation.id == active.tab_id
                    && observation.job_id == active.claim.job().id()
                    && observation.instance == active.attribution.instance()
            });
        let Some(observation) = observation else {
            self.stop_locally_after_lost_receiver_ownership(&active);
            return;
        };

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
        } else if observation.exited {
            self.retry_exited_receiver_run(&active, &path);
        } else {
            self.receiver
                .store_durable_run(crate::tui::receiver::DurableReceiverRun::Active(active));
        }
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

    fn retry_exited_receiver_run(&mut self, active: &ActiveReceiverRun, path: &std::path::Path) {
        if let Err(error) = self.services.release_receiver_session(&active.attribution) {
            crate::logging::log(format!("receiver session cleanup failed: {error:#}"));
        }
        self.remove_exact_receiver_tab(active);
        let _ = std::fs::remove_file(path);
        match self.retry_receiver_owner_now(&active.claim, ReceiverLaunchFailure::Spawn) {
            Ok(Some(_)) => {}
            Ok(None) => crate::logging::log("receiver exited after claim ownership was lost"),
            Err(error) => {
                crate::logging::log(format!("receiver exit retry recording failed: {error:#}"));
            }
        }
    }

    fn stop_locally_after_lost_receiver_ownership(&mut self, active: &ActiveReceiverRun) {
        self.remove_exact_receiver_tab(active);
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
