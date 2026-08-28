use super::receiver_durable_support::{ReceiverClock, accept_email_job};
use super::receiver_recovery_native_cleanup::{
    NativeCleanupRun, conversation_binding, fixture_connection,
    launch_prior_bound_rotated_accepted_run, registration_actual_session, registration_count,
    seed_receiver_artifacts,
};
use super::*;

use crate::state::{ReceiverJobState, ReceiverSessionBinding};
use crate::tui::receiver::ReceiverCleanupBoundary;

struct FreshConflictAppFixture {
    app: App,
    db: Db,
    clock: ReceiverClock,
    transport: TransportRecording,
    run: NativeCleanupRun,
    later_job_id: crate::state::ReceiverJobId,
    artifacts: [PathBuf; 3],
}

impl FreshConflictAppFixture {
    fn new(temporary: &tempfile::TempDir, actual_conflict: bool) -> Self {
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(temporary, &cli, AgentKind::Codex);
        app.receiver.record_intent(true);
        let clock = ReceiverClock::new();
        app.services
            .replace_receiver_sync_runtime(Box::new(clock.clone()));
        let db = Db::open(app.context.workspace()).expect("state DB");
        let transport = TransportRecording::default();
        app.brain.replace_receiver_transport(transport.transport());
        let run = launch_prior_bound_rotated_accepted_run(
            &mut app,
            &db,
            &clock,
            "prior-bound fresh fallback",
            100,
            actual_conflict,
        );
        let later_job_id = accept_email_job(&app, &db, "later FIFO work", 200).job_id();
        let artifacts = seed_receiver_artifacts(&app, &run.instance);
        Self {
            app,
            db,
            clock,
            transport,
            run,
            later_job_id,
            artifacts,
        }
    }

    fn prior_session(&self) -> AgentSession {
        let (kind, session_id) = conversation_binding(&self.run);
        assert_eq!(kind.as_deref(), Some(AgentKind::Codex.as_str()));
        AgentSession::new(session_id.expect("retained prior session"))
            .expect("valid retained prior session")
    }

    fn session_lock(&self, session: &AgentSession) -> Option<i64> {
        fixture_connection(&self.run)
            .query_row(
                "SELECT locked_pid FROM brain_sessions
                 WHERE workspace_id = ?1 AND agent_kind = ?2
                   AND actor_id = ?3 AND channel = ?4
                   AND agent_session_id = ?5",
                rusqlite::params![
                    &self.run.workspace_id,
                    self.run.scope.agent_kind().as_str(),
                    self.run.scope.actor().user_id().as_str(),
                    self.run.scope.actor().channel().as_str(),
                    session.as_str(),
                ],
                |row| row.get(0),
            )
            .expect("load exact session lock")
    }

    fn assert_terminal_conflict(&self) {
        let terminal = self
            .db
            .receiver_job(self.run.job_id)
            .expect("load fresh-conflict job")
            .expect("fresh-conflict job");
        assert_eq!(terminal.state(), ReceiverJobState::Failed);
        assert_eq!(
            terminal.last_error(),
            Some("recovery-native-session-unavailable")
        );
        assert!(
            terminal.pending_unavailable_notice(),
            "durable notice migration must wait for exact cleanup acknowledgement"
        );
        assert_eq!(
            terminal.recovery_cleanup_instance(),
            Some(self.run.instance.as_str())
        );
        assert_eq!(
            terminal.recovery_cleanup_session_id(),
            Some(self.run.native.as_str())
        );
        let prior = self.prior_session();
        assert_eq!(
            self.db
                .receiver_conversation(self.run.conversation_id)
                .expect("load retained conversation")
                .expect("retained conversation")
                .binding(),
            Some(
                &ReceiverSessionBinding::new(AgentKind::Codex, prior.as_str())
                    .expect("expected prior binding")
            )
        );
        assert_eq!(self.session_lock(&prior), None);
        assert!(self.session_lock(&self.run.native).is_some());
    }

    fn finish_local_acknowledgement(&mut self) {
        self.app.receiver.record_intent(false);
        self.app.tick_receiver();
        self.app.tick_receiver();
        assert_eq!(registration_count(&self.run), 0);
        assert_eq!(self.session_lock(&self.run.native), None);
        self.db
            .reconcile_expired_receiver_deliveries(self.clock.unix_ms())
            .expect("migrate notice after cleanup acknowledgement");
        let terminal = self
            .db
            .receiver_job(self.run.job_id)
            .expect("reload fresh-conflict job")
            .expect("fresh-conflict job after cleanup");
        assert_eq!(terminal.state(), ReceiverJobState::AnswerReady);
        assert!(!terminal.pending_unavailable_notice());
        assert!(
            self.db.receiver_delivery_counts().unwrap().answer_ready() == 1,
            "fresh-conflict cleanup changed the answer-ready count"
        );
        let prior = self.prior_session();
        assert_eq!(self.session_lock(&prior), None);
        assert!(
            SessionStore::claim(
                &self.db,
                &prior,
                "prior-session-remains-usable",
                77,
                &self.run.scope,
            )
            .expect("claim preserved prior session")
        );
        SessionStore::release(&self.db, "prior-session-remains-usable")
            .expect("release preserved prior session");
    }

    fn assert_later_fifo_advances(&self) {
        let next = self
            .db
            .claim_next_receiver_run(
                "later-after-fresh-conflict",
                self.clock.unix_ms(),
                self.clock.unix_ms() + 30_000,
            )
            .expect("claim later FIFO work")
            .expect("fresh conflict cannot fail stuck");
        assert_eq!(next.job().id(), self.later_job_id);
    }
}

#[test]
fn prior_bound_fresh_fallback_stall_cleans_exact_run_and_advances_fifo() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = FreshConflictAppFixture::new(&temporary, false);
    fixture
        .app
        .receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Acknowledgement);
    fixture
        .clock
        .advance(std::time::Duration::from_secs(5 * 60));

    fixture.app.tick_receiver();

    assert_eq!(fixture.transport.shutdowns(), 1);
    assert!(fixture.artifacts.iter().all(|path| !path.exists()));
    fixture.assert_terminal_conflict();
    assert_eq!(
        registration_actual_session(&fixture.run.state_path, &fixture.run.instance),
        None
    );
    fixture.finish_local_acknowledgement();
    fixture.assert_later_fifo_advances();
}

#[test]
fn prior_bound_fresh_fallback_absolute_expiry_cleans_exact_run_and_advances_fifo() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = FreshConflictAppFixture::new(&temporary, false);
    fixture_connection(&fixture.run)
        .execute(
            "UPDATE receiver_jobs SET absolute_work_expires_at_unix_ms = ?1
             WHERE workspace_id = ?2 AND job_id = ?3 AND job_token = ?4",
            rusqlite::params![
                fixture.clock.unix_ms(),
                &fixture.run.workspace_id,
                fixture.run.job_id.to_string(),
                fixture.run.token.to_string(),
            ],
        )
        .expect("expire exact prior-bound fresh fallback");
    fixture
        .app
        .receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Acknowledgement);

    fixture.app.tick_receiver();

    assert_eq!(fixture.transport.shutdowns(), 1);
    assert!(fixture.artifacts.iter().all(|path| !path.exists()));
    fixture.assert_terminal_conflict();
    fixture.finish_local_acknowledgement();
    fixture.assert_later_fifo_advances();
}

#[test]
fn conflicting_registration_actual_is_preserved_until_app_cleanup_acknowledges() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = FreshConflictAppFixture::new(&temporary, true);
    let prior = fixture.prior_session();
    fixture
        .app
        .receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Acknowledgement);
    fixture
        .clock
        .advance(std::time::Duration::from_secs(5 * 60));

    fixture.app.tick_receiver();

    fixture.assert_terminal_conflict();
    assert_eq!(
        registration_actual_session(&fixture.run.state_path, &fixture.run.instance).as_deref(),
        Some(prior.as_str())
    );
    fixture.finish_local_acknowledgement();
    fixture.assert_later_fifo_advances();
}

#[test]
fn restarted_app_releases_only_exact_dead_fresh_conflict_cleanup() {
    for actual_conflict in [false, true] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut fixture = FreshConflictAppFixture::new(&temporary, actual_conflict);
        fixture
            .app
            .receiver
            .inject_cleanup_failure(ReceiverCleanupBoundary::Acknowledgement);
        fixture
            .clock
            .advance(std::time::Duration::from_secs(5 * 60));
        fixture.app.tick_receiver();
        fixture.assert_terminal_conflict();
        fixture_connection(&fixture.run)
            .execute(
                "UPDATE brain_sessions SET locked_pid = 999999
                 WHERE workspace_id = ?1 AND brain_instance_id = ?2
                   AND agent_kind = ?3 AND actor_id = ?4 AND channel = ?5
                   AND agent_session_id = ?6",
                rusqlite::params![
                    &fixture.run.workspace_id,
                    &fixture.run.instance,
                    fixture.run.scope.agent_kind().as_str(),
                    fixture.run.scope.actor().user_id().as_str(),
                    fixture.run.scope.actor().channel().as_str(),
                    fixture.run.native.as_str(),
                ],
            )
            .expect("mark exact fresh lifecycle row dead");
        let prior_session_id = fixture.prior_session().as_str().to_owned();
        let run = fixture.run;
        let later_job_id = fixture.later_job_id;
        let clock = fixture.clock;
        drop(fixture.app);

        let cli = Cli::parse_from(["tasks"]);
        let mut restarted = test_app(&temporary, &cli, AgentKind::Codex);
        restarted.receiver.record_intent(true);
        restarted
            .services
            .replace_receiver_sync_runtime(Box::new(clock.clone()));
        restarted.tick_receiver();

        assert_eq!(registration_count(&run), 0);
        let db = Db::open(restarted.context.workspace()).expect("reopened state DB");
        let cleaned = db
            .receiver_job(run.job_id)
            .expect("load restarted fresh-conflict job")
            .expect("restarted fresh-conflict job");
        assert_eq!(cleaned.recovery_cleanup_instance(), None);
        assert_eq!(cleaned.recovery_cleanup_session_id(), None);
        assert_eq!(conversation_binding(&run).1, Some(prior_session_id));
        let later = db
            .receiver_job(later_job_id)
            .expect("load later work after restart cleanup")
            .expect("later work after restart cleanup");
        assert_ne!(later.state(), ReceiverJobState::Queued);
    }
}
