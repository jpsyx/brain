use super::receiver_durable_support::{ReceiverClock, accept_email_job};
use super::*;

use crate::state::{ReceiverAttemptKind, ReceiverJobState, ReceiverSessionBinding};
use crate::tui::receiver::{ReceiverCleanupBoundary, ReceiverLaunchBoundary};

pub(super) struct DueRecoveryFixture {
    pub(super) app: App,
    pub(super) db: Db,
    pub(super) clock: ReceiverClock,
    pub(super) accepted: crate::state::ReceiverAcceptance,
    pub(super) token: crate::state::ReceiverJobToken,
    pub(super) session: AgentSession,
    _transcript: ClaudeTranscript,
}

impl DueRecoveryFixture {
    pub(super) fn new(temporary: &tempfile::TempDir) -> Self {
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(temporary, &cli, AgentKind::Claude);
        app.receiver.record_intent(true);
        let clock = ReceiverClock::new();
        app.services
            .replace_receiver_sync_runtime(Box::new(clock.clone()));
        let db = Db::open(app.context.workspace()).expect("state DB");
        let accepted = accept_email_job(&app, &db, "private inbound must not replay", 100);
        let token = db
            .receiver_job(accepted.job_id())
            .expect("load accepted recovery job")
            .expect("accepted recovery job")
            .token();
        let session =
            AgentSession::new(uuid::Uuid::new_v4().to_string()).expect("native recovery session");
        let transcript = ClaudeTranscript::create(app.context.workspace().root(), session.as_str());
        let scope = SessionScope::new(
            AgentKind::Claude,
            app.context.workspace().id(),
            email_actor(),
        );
        SessionStore::register(&db, &session, "prior-recovery-owner", 41, &scope)
            .expect("register resumable session");
        SessionStore::release(&db, "prior-recovery-owner")
            .expect("leave resumable session unlocked");
        let binding = ReceiverSessionBinding::new(AgentKind::Claude, session.as_str())
            .expect("native recovery binding");
        db.update_receiver_conversation(
            accepted.conversation_id(),
            "portable transcript",
            Some(&binding),
            clock.unix_ms(),
        )
        .expect("bind recovery conversation");
        rusqlite::Connection::open(app.context.state_db_path())
            .expect("recovery fixture connection")
            .execute(
                "UPDATE receiver_jobs
                 SET state = 'retrying', attempt_kind = 'recovery', recovery_count = 1,
                     retry_at_unix_ms = ?1, retry_from_state = 'processing',
                     recovery_expires_at_unix_ms = ?2,
                     absolute_work_expires_at_unix_ms = ?3,
                     claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                     observation_instance = NULL, observation_session_id = NULL,
                     observation_revision = 0,
                     attempt_accepted_at_unix_ms = NULL,
                     attempt_progressing_at_unix_ms = NULL,
                     latest_progress_at_unix_ms = NULL,
                     recovery_cleanup_instance = NULL,
                     recovery_cleanup_session_id = NULL
                 WHERE workspace_id = ?4 AND job_id = ?5",
                rusqlite::params![
                    clock.unix_ms(),
                    clock.unix_ms() + 300_000,
                    clock.unix_ms() + 1_800_000,
                    app.context.workspace().id().to_string(),
                    accepted.job_id().to_string(),
                ],
            )
            .expect("persist due recovery fixture");
        Self {
            app,
            db,
            clock,
            accepted,
            token,
            session,
            _transcript: transcript,
        }
    }

    fn inject_owner_store_failure(&mut self, boundary: ReceiverLaunchBoundary) {
        let path = self.app.context.state_db_path().to_path_buf();
        self.app.receiver.install_launch_boundary_hook(
            boundary,
            Box::new(move || {
                rusqlite::Connection::open(path)
                    .expect("owner-store failure connection")
                    .execute_batch(
                        "ALTER TABLE receiver_jobs
                         RENAME TO receiver_jobs_recovery_store_unavailable;",
                    )
                    .expect("inject owner-store unavailability");
            }),
        );
    }

    fn restore_store(&self) {
        rusqlite::Connection::open(self.app.context.state_db_path())
            .expect("owner-store restore connection")
            .execute_batch(
                "ALTER TABLE receiver_jobs_recovery_store_unavailable
                 RENAME TO receiver_jobs;",
            )
            .expect("restore owner store");
    }

    fn assert_exact_recovery_retries(self) {
        let mut fixture = self;
        let transport = TransportRecording::default();
        fixture
            .app
            .brain
            .replace_receiver_transport(transport.transport());
        fixture.app.tick_receiver();

        assert_eq!(transport.launch_specs().len(), 1);
        let specification = &transport.launch_specs()[0];
        assert!(specification.command.contains("--resume"));
        assert!(specification.command.contains(fixture.session.as_str()));
        assert!(
            !specification
                .command
                .contains("private inbound must not replay")
        );
        let job = fixture
            .db
            .receiver_job(fixture.accepted.job_id())
            .expect("load retried recovery")
            .expect("retried recovery");
        assert_eq!(job.state(), ReceiverJobState::Launched);
        assert_eq!(job.attempt_kind(), ReceiverAttemptKind::Recovery);
        assert_eq!(job.recovery_count(), 1);
    }
}

fn store_unavailable_after_boundary_retries_exact_recovery(boundary: ReceiverLaunchBoundary) {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = DueRecoveryFixture::new(&temporary);
    let first_transport = TransportRecording::default();
    fixture
        .app
        .brain
        .replace_receiver_transport(first_transport.transport());
    fixture.inject_owner_store_failure(boundary);

    fixture.app.tick_receiver();

    assert!(first_transport.launch_specs().is_empty());
    fixture.restore_store();
    let claimed = fixture
        .app
        .receiver
        .recovery_claimed_durable_run()
        .expect("store ambiguity must retain the exact recovery claim");
    assert_eq!(claimed.claim.job().id(), fixture.accepted.job_id());
    assert_eq!(
        claimed.claim.job().attempt_kind(),
        ReceiverAttemptKind::Recovery
    );
    fixture.assert_exact_recovery_retries();
}

macro_rules! pre_spawn_store_unavailable_case {
    ($name:ident, $boundary:expr) => {
        #[test]
        fn $name() {
            store_unavailable_after_boundary_retries_exact_recovery($boundary);
        }
    };
}

pre_spawn_store_unavailable_case!(
    capability_planning_store_unavailable_retries_exact_recovery,
    ReceiverLaunchBoundary::CapabilityPlanning
);
pre_spawn_store_unavailable_case!(
    availability_store_unavailable_retries_exact_recovery,
    ReceiverLaunchBoundary::AvailabilityProbe
);
pre_spawn_store_unavailable_case!(
    resume_validation_store_unavailable_retries_exact_recovery,
    ReceiverLaunchBoundary::ResumeValidation
);
pre_spawn_store_unavailable_case!(
    registration_store_unavailable_retries_exact_recovery,
    ReceiverLaunchBoundary::Registration
);
pre_spawn_store_unavailable_case!(
    final_authorization_store_unavailable_retries_exact_recovery,
    ReceiverLaunchBoundary::RecoveryPreLaunchAuthorization
);
pre_spawn_store_unavailable_case!(
    launch_preparation_store_unavailable_retries_exact_recovery,
    ReceiverLaunchBoundary::RecoveryLaunchPreparation
);

#[test]
fn pre_spawn_cleanup_uncertainty_retains_exact_recovery_authority() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut fixture = DueRecoveryFixture::new(&temporary);
    let transport = TransportRecording::default();
    fixture
        .app
        .brain
        .replace_receiver_transport(transport.transport());
    fixture.inject_owner_store_failure(ReceiverLaunchBoundary::Registration);
    fixture
        .app
        .receiver
        .inject_cleanup_failure(ReceiverCleanupBoundary::Shutdown);

    fixture.app.tick_receiver();

    assert_eq!(transport.launch_specs().len(), 0);
    assert_eq!(transport.shutdowns(), 0);
    fixture.restore_store();

    fixture.app.tick_receiver();
    fixture.app.tick_receiver();
    let retry_transport = TransportRecording::default();
    fixture
        .app
        .brain
        .replace_receiver_transport(retry_transport.transport());
    fixture.app.tick_receiver();

    assert_eq!(transport.shutdowns(), 1);
    assert_eq!(retry_transport.launch_specs().len(), 1);
    assert!(
        retry_transport.launch_specs()[0]
            .command
            .contains(fixture.session.as_str())
    );
}
