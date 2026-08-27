use anyhow::Result;

use crate::agent::{AgentSession, CompletionStatus, SessionScope, SessionStore};
use crate::command::server::{ReceiverActionOutcome, ReceiverIntentRefresher};
use crate::state::{Db, PanelSide};
use crate::sync::args::Direction;
use crate::tui::app_sync::ReceiverSyncRuntime;
#[cfg(test)]
use crate::tui::receiver::attachments::ReceiverAttachmentRuntime;
use crate::tui::receiver::attachments::{ReceiverAttachmentCoordinator, ReceiverAttachmentEffect};
use crate::tui::shell::ShellRunner;
use crate::workspace::{CommandContext, ReceiverAction};

mod receiver_notice;
mod receiver_recovery;

pub(crate) use receiver_notice::ReceiverNoticeDelivery;
use receiver_notice::SystemReceiverNoticeDelivery;

pub(crate) struct AppServicesInit {
    pub(crate) agenda_runner: Box<dyn ShellRunner>,
    pub(crate) open_runner: Box<dyn ShellRunner>,
    pub(crate) db: Db,
    pub(crate) receiver_intent_refresher: Box<dyn ReceiverIntentRefresher>,
    pub(crate) receiver_sync_runtime: Box<dyn ReceiverSyncRuntime>,
}

pub(crate) struct AppServices {
    agenda_runner: Box<dyn ShellRunner>,
    open_runner: Box<dyn ShellRunner>,
    db: Db,
    receiver_intent_refresher: Box<dyn ReceiverIntentRefresher>,
    receiver_sync_runtime: Box<dyn ReceiverSyncRuntime>,
    receiver_attachment_coordinator: ReceiverAttachmentCoordinator,
    receiver_notice_delivery: Box<dyn ReceiverNoticeDelivery>,
    #[cfg(test)]
    receiver_recovery_commit_visible_error: std::cell::Cell<bool>,
}

pub(crate) struct ReceiverObservationApplyOutcome {
    pub(crate) changed: bool,
    pub(crate) completed: bool,
}

impl AppServices {
    pub(crate) fn new(init: AppServicesInit) -> Self {
        Self {
            agenda_runner: init.agenda_runner,
            open_runner: init.open_runner,
            db: init.db,
            receiver_intent_refresher: init.receiver_intent_refresher,
            receiver_sync_runtime: init.receiver_sync_runtime,
            receiver_attachment_coordinator: ReceiverAttachmentCoordinator::system(),
            receiver_notice_delivery: Box::new(SystemReceiverNoticeDelivery),
            #[cfg(test)]
            receiver_recovery_commit_visible_error: std::cell::Cell::new(false),
        }
    }

    pub(crate) fn run_agenda(&self) -> Result<()> {
        self.agenda_runner.run()
    }

    pub(crate) fn open_url(&self, url: &str) -> Result<()> {
        self.open_runner.open(url)
    }

    pub(crate) fn save_panel_side(&self, side: PanelSide) -> Result<()> {
        self.db.set_panel_side(side)
    }

    pub(crate) fn apply_receiver_action(
        &self,
        context: &CommandContext,
        action: ReceiverAction,
    ) -> Result<ReceiverActionOutcome> {
        crate::command::server::apply_receiver_action_with(
            context,
            action,
            self.receiver_intent_refresher.as_ref(),
        )
    }

    #[must_use]
    pub(crate) fn locked_session_for_instance(
        &self,
        instance: &str,
        scope: &SessionScope,
    ) -> Option<String> {
        self.db.locked_session_for_instance(instance, scope)
    }

    pub(crate) fn release_session_lock(&self, instance: &str) -> Result<()> {
        SessionStore::release(&self.db, instance)
    }

    pub(crate) fn register_receiver_session(
        &self,
        conversation_id: crate::state::ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<crate::state::ReceiverSessionAttribution> {
        self.db
            .register_receiver_session(conversation_id, session, instance, pid, scope)
    }

    pub(crate) fn claim_receiver_session(
        &self,
        conversation_id: crate::state::ReceiverConversationId,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<Option<crate::state::ReceiverSessionAttribution>> {
        self.db
            .claim_receiver_session(conversation_id, session, instance, pid, scope)
    }

    pub(crate) fn release_receiver_session(
        &self,
        registration: &crate::state::ReceiverSessionAttribution,
    ) -> Result<()> {
        self.db.release_receiver_session(registration)
    }

    pub(crate) fn claim_receiver_run(
        &self,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Option<crate::state::ReceiverRunClaim>> {
        self.db
            .claim_next_receiver_run(owner, now_unix_ms, expires_at_unix_ms)
    }

    pub(crate) fn complete_receiver_new_session(
        &self,
        job_id: crate::state::ReceiverJobId,
        owner: &str,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        self.db
            .complete_receiver_new_session(job_id, owner, observed_at_unix_ms)
    }

    pub(crate) fn apply_next_receiver_restart(
        &self,
        observed_at_unix_ms: u64,
    ) -> Result<Option<crate::server::receiver::RestartPlan<crate::server::receiver::InboundJob>>>
    {
        self.db.apply_next_receiver_restart(observed_at_unix_ms)
    }

    pub(crate) fn prepare_receiver_launch(
        &self,
        job_id: crate::state::ReceiverJobId,
        owner: &str,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        self.db
            .prepare_receiver_job_launch(job_id, owner, observed_at_unix_ms)
    }

    pub(crate) fn commit_receiver_job_launch(
        &self,
        job_id: crate::state::ReceiverJobId,
        owner: &str,
        observation: &crate::state::ReceiverLaunchObservation,
    ) -> Result<bool> {
        self.db
            .commit_receiver_job_launch(job_id, owner, observation)
    }

    pub(crate) fn poll_receiver_attachment_stage(
        &mut self,
        job_id: crate::state::ReceiverJobId,
        command: &CommandContext,
        message: &crate::server::receiver::InboundJob,
    ) -> ReceiverAttachmentEffect {
        self.receiver_attachment_coordinator
            .poll_or_start(job_id, command, message)
    }

    pub(crate) fn cancel_receiver_attachment_stage(&mut self, job_id: crate::state::ReceiverJobId) {
        self.receiver_attachment_coordinator.cancel(job_id);
    }

    pub(crate) fn shutdown_receiver_attachments(&mut self) {
        self.receiver_attachment_coordinator.shutdown();
    }

    pub(crate) fn renew_receiver_claim(
        &self,
        job_id: crate::state::ReceiverJobId,
        owner: &str,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<bool> {
        self.db
            .renew_receiver_claim(job_id, owner, now_unix_ms, expires_at_unix_ms)
    }

    pub(crate) fn receiver_observation_cursor(
        &self,
        job_id: crate::state::ReceiverJobId,
    ) -> Result<
        Option<(
            crate::state::ReceiverJobState,
            crate::agent::AgentObservationCursor,
        )>,
    > {
        self.db.receiver_job(job_id)?.map_or(Ok(None), |job| {
            Ok(Some((job.state(), job.observation_cursor()?)))
        })
    }

    pub(crate) fn apply_receiver_observation_result(
        &self,
        job_id: crate::state::ReceiverJobId,
        token: crate::state::ReceiverJobToken,
        owner: &str,
        registration: &crate::state::ReceiverSessionAttribution,
        result: &crate::agent::AgentObservationResult,
        authorized_at_unix_ms: u64,
    ) -> Result<ReceiverObservationApplyOutcome> {
        let observation = crate::state::ReceiverObservationSet::from_agent_observation(
            token,
            registration,
            result,
            authorized_at_unix_ms,
        );
        let completed = result.is_completed();
        let changed = if completed {
            self.db.apply_terminal_receiver_observation_set(
                job_id,
                owner,
                &observation,
                registration,
                result.session(),
            )?
        } else {
            self.db
                .apply_receiver_observation_set(job_id, owner, &observation)?
        };
        Ok(ReceiverObservationApplyOutcome { changed, completed })
    }

    pub(crate) fn receiver_observation_set(
        token: crate::state::ReceiverJobToken,
        registration: &crate::state::ReceiverSessionAttribution,
        result: &crate::agent::AgentObservationResult,
        authorized_at_unix_ms: u64,
    ) -> crate::state::ReceiverObservationSet {
        crate::state::ReceiverObservationSet::from_agent_observation(
            token,
            registration,
            result,
            authorized_at_unix_ms,
        )
    }

    pub(crate) fn complete_receiver_job_with_observation(
        &self,
        request: &crate::state::ReceiverCompletionRequest<'_>,
        observation: Option<&crate::state::ReceiverObservationSet>,
    ) -> Result<bool> {
        self.db
            .complete_receiver_job_with_observation(request, observation)
    }

    pub(crate) fn record_receiver_launch_retry(
        &self,
        job_id: crate::state::ReceiverJobId,
        owner: &str,
        observed_at_unix_ms: u64,
        retry_at_unix_ms: u64,
        failure: crate::state::ReceiverLaunchFailure,
    ) -> Result<Option<crate::state::ReceiverLaunchRetryOutcome>> {
        self.db.record_receiver_launch_retry(
            job_id,
            owner,
            observed_at_unix_ms,
            retry_at_unix_ms,
            failure,
        )
    }

    #[must_use]
    pub(crate) fn monotonic_now(&self) -> std::time::Instant {
        self.receiver_sync_runtime.monotonic_now()
    }

    #[must_use]
    pub(crate) fn utc_now(&self) -> chrono::DateTime<chrono::Utc> {
        self.receiver_sync_runtime.utc_now()
    }

    #[must_use]
    pub(crate) fn live_sync_state(
        &self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<crate::sync::current::CurrentState> {
        self.receiver_sync_runtime.live_sync_state(paths)
    }

    #[must_use]
    pub(crate) fn latest_successful_downstream_id(
        &self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<i64> {
        self.receiver_sync_runtime
            .latest_successful_downstream_id(paths)
    }

    #[must_use]
    pub(crate) fn latest_downstream_completion(
        &self,
        paths: &crate::workspace::WorkspacePaths,
    ) -> Option<String> {
        self.receiver_sync_runtime
            .latest_downstream_completion(paths)
    }

    #[must_use]
    pub(crate) fn spawn_detached_sync(
        &self,
        workspace: &crate::workspace::WorkspaceContext,
        direction: Direction,
    ) -> Option<u32> {
        self.receiver_sync_runtime
            .spawn_detached_sync(workspace, direction)
    }

    #[cfg(test)]
    pub(crate) fn replace_receiver_sync_runtime(&mut self, runtime: Box<dyn ReceiverSyncRuntime>) {
        self.receiver_sync_runtime = runtime;
    }

    #[cfg(test)]
    pub(crate) fn replace_receiver_attachment_runtime(
        &mut self,
        runtime: Box<dyn ReceiverAttachmentRuntime>,
    ) {
        self.receiver_attachment_coordinator.replace(runtime);
    }

    #[cfg(test)]
    pub(crate) fn replace_receiver_intent_refresher(
        &mut self,
        refresher: Box<dyn ReceiverIntentRefresher>,
    ) {
        self.receiver_intent_refresher = refresher;
    }
}

impl SessionStore for AppServices {
    fn reap_dead_locks(&self) -> Result<()> {
        SessionStore::reap_dead_locks(&self.db)
    }

    fn sessions_by_recency(&self, scope: &SessionScope) -> Vec<String> {
        SessionStore::sessions_by_recency(&self.db, scope)
    }

    fn claim(
        &self,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<bool> {
        SessionStore::claim(&self.db, session, instance, pid, scope)
    }

    fn register(
        &self,
        session: &AgentSession,
        instance: &str,
        pid: i32,
        scope: &SessionScope,
    ) -> Result<()> {
        SessionStore::register(&self.db, session, instance, pid, scope)
    }

    fn release(&self, instance: &str) -> Result<()> {
        SessionStore::release(&self.db, instance)
    }

    fn mark_active(&self, instance: &str, scope: &SessionScope) -> Result<bool> {
        SessionStore::mark_active(&self.db, instance, scope)
    }

    fn mark_completed(&self, session: &AgentSession, scope: &SessionScope) -> Result<bool> {
        SessionStore::mark_completed(&self.db, session, scope)
    }

    fn completion_status(
        &self,
        session: &AgentSession,
        scope: &SessionScope,
    ) -> Option<CompletionStatus> {
        SessionStore::completion_status(&self.db, session, scope)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use anyhow::Result;

    use crate::sync::args::Direction;
    use crate::tui::app_sync::ReceiverSyncRuntime;
    use crate::tui::shell::ShellRunner;

    use super::{AppServices, AppServicesInit};

    #[derive(Clone, Default)]
    struct RecordingRunner(Arc<Mutex<Vec<String>>>);

    impl ShellRunner for RecordingRunner {
        fn run(&self) -> Result<()> {
            self.0
                .lock()
                .expect("recording runner")
                .push("run".to_owned());
            Ok(())
        }

        fn open(&self, url: &str) -> Result<()> {
            self.0
                .lock()
                .expect("recording runner")
                .push(format!("open:{url}"));
            Ok(())
        }
    }

    struct FixedSyncRuntime {
        now: Instant,
    }

    impl ReceiverSyncRuntime for FixedSyncRuntime {
        fn monotonic_now(&self) -> Instant {
            self.now
        }

        fn utc_now(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::DateTime::UNIX_EPOCH
        }

        fn live_sync_state(
            &self,
            _paths: &crate::workspace::WorkspacePaths,
        ) -> Option<crate::sync::current::CurrentState> {
            None
        }

        fn latest_successful_downstream_id(
            &self,
            _paths: &crate::workspace::WorkspacePaths,
        ) -> Option<i64> {
            Some(17)
        }

        fn latest_downstream_completion(
            &self,
            _paths: &crate::workspace::WorkspacePaths,
        ) -> Option<String> {
            Some("2026-08-21T12:00:00Z".to_owned())
        }

        fn spawn_detached_sync(
            &self,
            _workspace: &crate::workspace::WorkspaceContext,
            _direction: Direction,
        ) -> Option<u32> {
            Some(42)
        }
    }

    #[test]
    fn services_preserve_focused_injected_runner_and_sync_behavior() {
        let agenda = RecordingRunner::default();
        let opener = RecordingRunner::default();
        let now = Instant::now();
        let services = AppServices::new(AppServicesInit {
            agenda_runner: Box::new(agenda.clone()),
            open_runner: Box::new(opener.clone()),
            db: crate::state::Db::open_in_memory().expect("state db"),
            receiver_intent_refresher: Box::new(crate::server::control::ServerClient::default()),
            receiver_sync_runtime: Box::new(FixedSyncRuntime { now }),
        });

        services.run_agenda().expect("agenda runner");
        services
            .open_url("https://example.test/issue/BR-11")
            .expect("URL opener");

        assert_eq!(*agenda.0.lock().expect("agenda calls"), ["run"]);
        assert_eq!(
            *opener.0.lock().expect("open calls"),
            ["open:https://example.test/issue/BR-11"]
        );
        assert_eq!(services.monotonic_now(), now);
    }
}
