use super::receiver_durable_support::{accept_email_job, publish_valid_completion};
use super::receiver_sync::configure_receiver_sync;
use super::*;

use crate::state::ReceiverJobState;
use crate::tui::receiver::{ReceiverAnswerCleanupEvent, ReceiverCleanupBoundary};
use rusqlite::OptionalExtension as _;

#[test]
fn session_release_failure_retains_cleanup_without_blocking_the_next_job() {
    let (_temporary, mut app, db, first, second, transport) = answer_fixture();
    configure_receiver_sync(&app);
    let sync = CompletionSyncRuntime::new(true);
    app.services
        .replace_receiver_sync_runtime(Box::new(sync.clone()));
    let artifact = publish_valid_completion(&app, "answer survives session release failure");
    app.receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Session);

    app.tick_receiver();

    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );
    assert_eq!(delivery_count(&app, first.job_id()), 1);
    assert!(!artifact.exists());
    assert_eq!(transport.shutdowns(), 1);
    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(answer_cleanup_flags(&app, first.job_id()), Some((0, 1)));
    assert_eq!(registration_count(&app), 1);
    assert_eq!(sync.pushes(), 0, "sync must wait for exact session release");

    app.brain
        .replace_receiver_transport(TransportRecording::default().transport());
    app.tick_receiver();

    assert_eq!(answer_cleanup_flags(&app, first.job_id()), None);
    assert_eq!(
        registration_count(&app),
        1,
        "the next job owns one registration"
    );
    assert_eq!(sync.pushes(), 1);
    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        second.job_id()
    );
}

#[test]
fn artifact_cleanup_failure_is_retried_by_a_fresh_app_before_final_sync() {
    let (temporary, mut app, db, first, second, transport) = answer_fixture();
    configure_receiver_sync(&app);
    let first_sync = CompletionSyncRuntime::new(true);
    app.services
        .replace_receiver_sync_runtime(Box::new(first_sync.clone()));
    let artifact = publish_valid_completion(&app, "answer survives cleanup restart");
    app.receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Artifacts);

    app.tick_receiver();

    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );
    assert!(artifact.exists());
    assert_eq!(transport.shutdowns(), 1);
    assert_eq!(answer_cleanup_flags(&app, first.job_id()), Some((1, 0)));
    assert_eq!(registration_count(&app), 0);
    assert_eq!(
        first_sync.pushes(),
        0,
        "sync must wait for artifact cleanup"
    );
    drop(app);

    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(&temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    configure_receiver_sync(&restarted);
    let restart_sync = CompletionSyncRuntime::new(true);
    restarted
        .services
        .replace_receiver_sync_runtime(Box::new(restart_sync.clone()));
    restarted
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());

    restarted.tick_receiver();

    assert!(!artifact.exists());
    assert_eq!(answer_cleanup_flags(&restarted, first.job_id()), None);
    assert_eq!(restart_sync.pushes(), 1);
    assert_eq!(
        restarted.brain.receiver_run_observations()[0].job_id,
        second.job_id()
    );
}

#[test]
fn completion_sync_start_failure_is_post_commit_and_content_free() {
    let (_temporary, mut app, db, first, _second, transport) = answer_fixture();
    configure_receiver_sync(&app);
    let sync = CompletionSyncRuntime::new(false);
    app.services
        .replace_receiver_sync_runtime(Box::new(sync.clone()));
    let artifact = publish_valid_completion(&app, "answer survives sync start failure");

    app.tick_receiver();

    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );
    assert_eq!(completion_evidence_count(&app, first.job_id()), 1);
    assert_eq!(sync.pushes(), 1);
    assert!(!artifact.exists());
    assert_eq!(transport.shutdowns(), 1);
    let diagnostic = app
        .receiver
        .last_observation_diagnostic()
        .expect("sync failure diagnostic");
    assert!(diagnostic.ends_with("category=completion-sync-start"));
    assert!(!diagnostic.contains("answer survives"));
}

#[test]
fn successful_answer_commit_runs_each_post_commit_effect_once_then_launches_next() {
    let (_temporary, mut app, db, first, second, transport) = answer_fixture();
    configure_receiver_sync(&app);
    let sync = CompletionSyncRuntime::new(true);
    app.services
        .replace_receiver_sync_runtime(Box::new(sync.clone()));
    std::fs::write(
        app.context.tasks_csv_path(),
        "task_uuid,task_id,task_name,task_type,status,priority,due_date,hard_deadline,start_date,assigned_to,system_key,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue\n\
         8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,T1,Reloaded after answer,,not_started,p2,,false,,pablo,,,,,,,,,0,2026-08-24,,,\n",
    )
    .expect("replace task fixture");
    let artifact = publish_valid_completion(&app, "exact post-commit answer");

    app.tick_receiver();

    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );
    assert_eq!(completion_evidence_count(&app, first.job_id()), 1);
    assert_eq!(sync.pushes(), 1);
    assert_eq!(transport.shutdowns(), 1);
    assert!(app.brain.receiver_run_observations().is_empty());
    assert!(!artifact.exists());
    assert!(app.tasks.contains_task_named("Reloaded after answer"));
    let registrations: i64 = rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state for registration count")
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations",
            [],
            |row| row.get(0),
        )
        .expect("remaining receiver registrations");
    assert_eq!(registrations, 0);

    app.brain
        .replace_receiver_transport(TransportRecording::default().transport());
    app.tick_receiver();
    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        second.job_id()
    );
}

#[test]
fn final_sync_starts_after_every_private_cleanup_boundary_in_order() {
    let (_temporary, mut app, db, first, _second, transport) = answer_fixture();
    configure_receiver_sync(&app);
    let sync = CompletionSyncRuntime::new(true);
    app.services
        .replace_receiver_sync_runtime(Box::new(sync.clone()));
    let artifact = publish_valid_completion(&app, "ordered private cleanup answer");

    app.tick_receiver();

    assert_eq!(
        job_state(&db, first.job_id()),
        ReceiverJobState::AnswerReady
    );
    assert_eq!(transport.shutdowns(), 1);
    assert!(!artifact.exists());
    assert_eq!(registration_count(&app), 0);
    assert_eq!(sync.pushes(), 1);
    assert_eq!(
        app.receiver.answer_cleanup_events(),
        &[
            ReceiverAnswerCleanupEvent::ControllerShutdown,
            ReceiverAnswerCleanupEvent::SessionRelease,
            ReceiverAnswerCleanupEvent::ArtifactCleanup,
            ReceiverAnswerCleanupEvent::TaskReload,
            ReceiverAnswerCleanupEvent::SyncLaunch,
        ]
    );
}

pub(super) fn answer_fixture() -> (
    tempfile::TempDir,
    App,
    Db,
    crate::state::ReceiverAcceptance,
    crate::state::ReceiverAcceptance,
    TransportRecording,
) {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let first = accept_email_job(&app, &db, "first answer", 100);
    let second = accept_email_job(&app, &db, "second answer", 200);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    (temporary, app, db, first, second, transport)
}

pub(super) fn job_state(db: &Db, job_id: crate::state::ReceiverJobId) -> ReceiverJobState {
    db.receiver_job(job_id)
        .expect("load receiver job")
        .expect("durable receiver job")
        .state()
}

pub(super) fn delivery_count(app: &App, job_id: crate::state::ReceiverJobId) -> i64 {
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state for delivery count")
        .query_row(
            "SELECT COUNT(*) FROM receiver_deliveries
             WHERE job_id = ?1 AND response_kind = 'final-answer'",
            [job_id.to_string()],
            |row| row.get(0),
        )
        .expect("final-answer delivery count")
}

pub(super) fn completion_evidence_count(app: &App, job_id: crate::state::ReceiverJobId) -> i64 {
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state for evidence count")
        .query_row(
            "SELECT COUNT(*) FROM receiver_deliveries
             WHERE job_id = ?1 AND response_kind = 'final-answer'
               AND completion_evidence_json IS NOT NULL",
            [job_id.to_string()],
            |row| row.get(0),
        )
        .expect("immutable completion evidence count")
}

fn answer_cleanup_flags(app: &App, job_id: crate::state::ReceiverJobId) -> Option<(i64, i64)> {
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state for cleanup flags")
        .query_row(
            "SELECT session_released, artifacts_removed
             FROM receiver_answer_cleanups WHERE job_id = ?1",
            [job_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .expect("load answer cleanup flags")
}

fn registration_count(app: &App) -> i64 {
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state for registration count")
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations",
            [],
            |row| row.get(0),
        )
        .expect("remaining receiver registrations")
}

#[derive(Clone)]
pub(super) struct CompletionSyncRuntime {
    pushes: Arc<Mutex<usize>>,
    starts: bool,
}

impl CompletionSyncRuntime {
    pub(super) fn new(starts: bool) -> Self {
        Self {
            pushes: Arc::new(Mutex::new(0)),
            starts,
        }
    }

    pub(super) fn pushes(&self) -> usize {
        *self.pushes.lock().expect("sync push count")
    }
}

impl crate::tui::app_sync::ReceiverSyncRuntime for CompletionSyncRuntime {
    fn monotonic_now(&self) -> std::time::Instant {
        std::time::Instant::now()
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
        None
    }

    fn latest_downstream_completion(
        &self,
        _paths: &crate::workspace::WorkspacePaths,
    ) -> Option<String> {
        Some("2026-08-27T00:00:00Z".to_owned())
    }

    fn spawn_detached_sync(
        &self,
        _workspace: &WorkspaceContext,
        direction: crate::sync::args::Direction,
    ) -> Option<u32> {
        if direction == crate::sync::args::Direction::Push {
            *self.pushes.lock().expect("record sync push") += 1;
        }
        self.starts.then_some(42)
    }
}
