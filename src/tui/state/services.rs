use anyhow::Result;

use crate::agent::{AgentSession, CompletionStatus, SessionScope, SessionStore};
use crate::state::{Db, PanelSide};
use crate::sync::args::Direction;
use crate::tui::app_sync::ReceiverSyncRuntime;
use crate::tui::shell::ShellRunner;

pub(crate) struct AppServicesInit {
    pub(crate) agenda_runner: Box<dyn ShellRunner>,
    pub(crate) open_runner: Box<dyn ShellRunner>,
    pub(crate) db: Db,
    pub(crate) receiver_sync_runtime: Box<dyn ReceiverSyncRuntime>,
}

pub(crate) struct AppServices {
    agenda_runner: Box<dyn ShellRunner>,
    open_runner: Box<dyn ShellRunner>,
    db: Db,
    receiver_sync_runtime: Box<dyn ReceiverSyncRuntime>,
}

impl AppServices {
    pub(crate) fn new(init: AppServicesInit) -> Self {
        Self {
            agenda_runner: init.agenda_runner,
            open_runner: init.open_runner,
            db: init.db,
            receiver_sync_runtime: init.receiver_sync_runtime,
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
