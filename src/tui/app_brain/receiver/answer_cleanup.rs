//! Retryable local effects after an answer has released agent ownership.

use crate::state::{ReceiverAnswerCleanup, ReceiverJobId};
use crate::tui::App;
use crate::tui::receiver::{ActiveReceiverRun, DurableReceiverRun};

impl App {
    pub(super) fn continue_oldest_receiver_answer_cleanup(&mut self) {
        match self.services.next_receiver_answer_cleanup() {
            Ok(Some(cleanup)) => self.continue_receiver_answer_cleanup(cleanup),
            Ok(None) => {}
            Err(_) => crate::logging::log("receiver answer cleanup failed boundary=load"),
        }
    }

    pub(super) fn begin_receiver_answer_cleanup(&mut self, active: ActiveReceiverRun) {
        match self.brain.shutdown_receiver_run(
            active.tab_id,
            active.claim.job().id(),
            active.attribution.instance(),
        ) {
            Ok(true) => {
                let controller_pid = i32::try_from(std::process::id()).unwrap_or(0);
                if !self
                    .services
                    .acknowledge_receiver_answer_controller_shutdown(
                        active.claim.job().id(),
                        active.claim.job().token(),
                        active.attribution.instance(),
                        controller_pid,
                        self.receiver_now_unix_ms(),
                    )
                    .unwrap_or(false)
                {
                    self.defer_receiver_answer_controller_cleanup(active);
                    return;
                }
                if self
                    .brain
                    .remove_shutdown_receiver_run(
                        active.tab_id,
                        active.claim.job().id(),
                        active.attribution.instance(),
                    )
                    .is_none()
                {
                    self.defer_receiver_answer_controller_cleanup(active);
                    return;
                }
                #[cfg(test)]
                self.receiver.record_answer_cleanup_event(
                    crate::tui::receiver::ReceiverAnswerCleanupEvent::ControllerShutdown,
                );
                self.continue_receiver_answer_cleanup_for(active.claim.job().id());
            }
            Ok(false) | Err(_) => self.defer_receiver_answer_controller_cleanup(active),
        }
    }

    pub(super) fn continue_receiver_answer_controller_cleanup(
        &mut self,
        active: ActiveReceiverRun,
    ) {
        self.begin_receiver_answer_cleanup(active);
    }

    fn continue_receiver_answer_cleanup_for(&mut self, job_id: ReceiverJobId) {
        match self.services.receiver_answer_cleanup(job_id) {
            Ok(Some(cleanup)) => self.continue_receiver_answer_cleanup(cleanup),
            Ok(None) => {}
            Err(_) => crate::logging::log("receiver answer cleanup failed boundary=load-exact"),
        }
    }

    fn continue_receiver_answer_cleanup(&mut self, mut cleanup: ReceiverAnswerCleanup) {
        if !cleanup.session_released() {
            #[cfg(test)]
            let released = if self
                .receiver
                .take_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Session)
            {
                false
            } else {
                self.services
                    .release_receiver_answer_cleanup_session(&cleanup, self.receiver_now_unix_ms())
                    .unwrap_or(false)
            };
            #[cfg(not(test))]
            let released = self
                .services
                .release_receiver_answer_cleanup_session(&cleanup, self.receiver_now_unix_ms())
                .unwrap_or(false);
            if released {
                #[cfg(test)]
                self.receiver.record_answer_cleanup_event(
                    crate::tui::receiver::ReceiverAnswerCleanupEvent::SessionRelease,
                );
                cleanup = match self.services.receiver_answer_cleanup(cleanup.job_id()) {
                    Ok(Some(cleanup)) => cleanup,
                    Ok(None) | Err(_) => return,
                };
            } else {
                crate::logging::log(format!(
                    "receiver answer cleanup incomplete job={} boundary=session-release",
                    cleanup.job_id()
                ));
            }
        }
        if !cleanup.artifacts_removed() {
            #[cfg(test)]
            let removed = if self
                .receiver
                .take_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts)
            {
                false
            } else {
                self.cleanup_receiver_instance_files_checked(cleanup.instance())
                    .is_ok()
            };
            #[cfg(not(test))]
            let removed = self
                .cleanup_receiver_instance_files_checked(cleanup.instance())
                .is_ok();
            if removed
                && self
                    .services
                    .mark_receiver_answer_artifacts_removed(&cleanup, self.receiver_now_unix_ms())
                    .unwrap_or(false)
            {
                #[cfg(test)]
                self.receiver.record_answer_cleanup_event(
                    crate::tui::receiver::ReceiverAnswerCleanupEvent::ArtifactCleanup,
                );
                cleanup = match self.services.receiver_answer_cleanup(cleanup.job_id()) {
                    Ok(Some(cleanup)) => cleanup,
                    Ok(None) | Err(_) => return,
                };
            } else {
                crate::logging::log(format!(
                    "receiver answer cleanup incomplete job={} boundary=artifacts",
                    cleanup.job_id()
                ));
            }
        }
        if !cleanup.session_released() || !cleanup.artifacts_removed() {
            return;
        }
        if self.reload_tasks().is_err() {
            crate::logging::log(format!(
                "receiver answer cleanup incomplete job={} boundary=task-reload",
                cleanup.job_id()
            ));
            return;
        }
        #[cfg(test)]
        self.receiver.record_answer_cleanup_event(
            crate::tui::receiver::ReceiverAnswerCleanupEvent::TaskReload,
        );
        let sync_configured =
            crate::sync::config::SyncConfig::load(self.context.command()).is_configured();
        if sync_configured
            && self
                .services
                .spawn_detached_sync(self.context.workspace(), crate::sync::args::Direction::Push)
                .is_none()
        {
            let diagnostic = receiver_answer_cleanup_diagnostic(&cleanup, "completion-sync-start");
            #[cfg(test)]
            self.receiver
                .record_observation_diagnostic(diagnostic.clone());
            crate::logging::log(diagnostic);
            return;
        }
        #[cfg(test)]
        if sync_configured {
            self.receiver.record_answer_cleanup_event(
                crate::tui::receiver::ReceiverAnswerCleanupEvent::SyncLaunch,
            );
        }
        if !self
            .services
            .finish_receiver_answer_cleanup(&cleanup)
            .unwrap_or(false)
        {
            crate::logging::log(format!(
                "receiver answer cleanup incomplete job={} boundary=finish",
                cleanup.job_id()
            ));
        }
    }

    fn defer_receiver_answer_controller_cleanup(&mut self, active: ActiveReceiverRun) {
        crate::logging::log(format!(
            "receiver answer cleanup incomplete job={} boundary=controller-shutdown",
            active.claim.job().id()
        ));
        self.receiver
            .store_durable_run(DurableReceiverRun::AnswerCleanupPending(active));
    }
}

fn receiver_answer_cleanup_diagnostic(
    cleanup: &ReceiverAnswerCleanup,
    category: &'static str,
) -> String {
    super::diagnostic::receiver_observation_diagnostic(
        cleanup.job_id(),
        cleanup.instance(),
        cleanup.frontend(),
        crate::state::ReceiverJobState::AnswerReady,
        None,
        category,
    )
}
