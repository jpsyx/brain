use super::receiver_durable_support::ReceiverClock;
use super::receiver_recovery_authority::DueRecoveryFixture;
use super::*;

use crate::tui::receiver::{ReceiverCleanupBoundary, ReceiverLaunchBoundary};

#[derive(Clone, Copy)]
enum RegistrationMismatch {
    JobToken,
    Actor,
    Channel,
    Frontend,
    NativeSession,
    Source,
    LockPid,
    RegistrationActual,
}

impl RegistrationMismatch {
    const ALL: [Self; 8] = [
        Self::JobToken,
        Self::Actor,
        Self::Channel,
        Self::Frontend,
        Self::NativeSession,
        Self::Source,
        Self::LockPid,
        Self::RegistrationActual,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::JobToken => "job-token",
            Self::Actor => "actor",
            Self::Channel => "channel",
            Self::Frontend => "frontend",
            Self::NativeSession => "native-session",
            Self::Source => "source",
            Self::LockPid => "lock-pid",
            Self::RegistrationActual => "registration-actual",
        }
    }
}

#[test]
fn incomplete_recovery_registration_proof_fails_closed_without_releasing_any_lock() {
    let mut released = Vec::new();
    for mismatch in RegistrationMismatch::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut fixture = DueRecoveryFixture::new(&temporary);
        fixture
            .app
            .brain
            .replace_receiver_transport(TransportRecording::default().transport());
        advance_clock_after_spawn(&mut fixture.app, fixture.clock.clone());
        fixture
            .app
            .receiver
            .inject_cleanup_failure(ReceiverCleanupBoundary::Shutdown);
        fixture.app.tick_receiver();
        let instance = fixture
            .app
            .receiver
            .spawned_recovery_durable_run()
            .expect("retained spawned recovery")
            .attribution
            .instance()
            .to_owned();
        let expected_pid = mutate_registration_proof(&fixture, &instance, mismatch);
        let deadline = fixture
            .db
            .receiver_job(fixture.accepted.job_id())
            .expect("load mismatched recovery")
            .expect("mismatched recovery")
            .launch_expires_at_unix_ms()
            .expect("launch deadline");
        fixture.clock.advance(std::time::Duration::from_millis(
            deadline.saturating_sub(fixture.clock.unix_ms()),
        ));
        fixture.app.tick_receiver();
        let connection = rusqlite::Connection::open(fixture.app.context.state_db_path())
            .expect("mismatch inspection connection");
        let locked_pid = connection
            .query_row(
                "SELECT locked_pid FROM brain_sessions WHERE brain_instance_id = ?1",
                [&instance],
                |row| row.get::<_, Option<i64>>(0),
            )
            .expect("load mismatched lock");
        let registrations = connection
            .query_row(
                "SELECT COUNT(*) FROM receiver_session_registrations
                 WHERE brain_instance_id = ?1",
                [&instance],
                |row| row.get::<_, i64>(0),
            )
            .expect("count mismatched registrations");
        if locked_pid != Some(expected_pid) || registrations != 1 {
            released.push(mismatch.label());
        }
    }

    assert!(
        released.is_empty(),
        "incomplete exact proof released registrations for {released:?}"
    );
}

fn advance_clock_after_spawn(app: &mut App, clock: ReceiverClock) {
    app.receiver.install_launch_boundary_hook(
        ReceiverLaunchBoundary::Spawn,
        Box::new(move || clock.advance(std::time::Duration::from_secs(31))),
    );
}

fn mutate_registration_proof(
    fixture: &DueRecoveryFixture,
    instance: &str,
    mismatch: RegistrationMismatch,
) -> i64 {
    let connection = rusqlite::Connection::open(fixture.app.context.state_db_path())
        .expect("mismatch fixture connection");
    match mismatch {
        RegistrationMismatch::JobToken => {
            connection
                .execute(
                    "UPDATE receiver_jobs SET job_token = ?1 WHERE job_id = ?2",
                    rusqlite::params![
                        uuid::Uuid::new_v4().to_string(),
                        fixture.accepted.job_id().to_string(),
                    ],
                )
                .expect("mismatch job token");
        }
        RegistrationMismatch::Actor => {
            update_registration_scope(&connection, instance, "actor_id", "wrong-actor");
        }
        RegistrationMismatch::Channel => {
            update_registration_scope(&connection, instance, "channel", "sms");
        }
        RegistrationMismatch::Frontend => {
            update_registration_scope(&connection, instance, "agent_kind", "codex");
        }
        RegistrationMismatch::NativeSession => {
            connection
                .execute(
                    "UPDATE brain_sessions SET agent_session_id = 'wrong-session'
                     WHERE brain_instance_id = ?1",
                    [instance],
                )
                .expect("mismatch native session");
        }
        RegistrationMismatch::Source => {
            connection
                .execute(
                    "UPDATE brain_sessions SET source = NULL
                     WHERE brain_instance_id = ?1",
                    [instance],
                )
                .expect("mismatch lifecycle source");
        }
        RegistrationMismatch::LockPid => {
            connection
                .execute(
                    "UPDATE brain_sessions SET locked_pid = 999999
                     WHERE brain_instance_id = ?1",
                    [instance],
                )
                .expect("mismatch lock PID");
            return 999_999;
        }
        RegistrationMismatch::RegistrationActual => {
            connection
                .execute(
                    "UPDATE receiver_session_registrations
                     SET actual_session_id = 'wrong-actual-session'
                     WHERE brain_instance_id = ?1",
                    [instance],
                )
                .expect("mismatch registration actual session");
        }
    }
    i64::from(i32::try_from(std::process::id()).unwrap_or(0))
}

fn update_registration_scope(
    connection: &rusqlite::Connection,
    instance: &str,
    column: &str,
    value: &str,
) {
    for table in ["brain_sessions", "receiver_session_registrations"] {
        connection
            .execute(
                &format!("UPDATE {table} SET {column} = ?1 WHERE brain_instance_id = ?2"),
                rusqlite::params![value, instance],
            )
            .expect("mismatch exact registration scope");
    }
}
