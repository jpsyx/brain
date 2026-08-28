use super::receiver_durable_producer_matrix::{
    active_completion_path, active_observation_path, produce_completion, rotate_active_session,
};
use super::receiver_durable_support::accept_email_job;
use super::*;

use crate::agent::AgentObservationCursor;
use crate::state::ReceiverJobState;

#[test]
fn saturated_accepted_stop_finishes_once_for_every_frontend() {
    assert_saturated_stop_finishes_once("accepted", None);
}

#[test]
fn saturated_progressing_stop_finishes_once_for_every_frontend() {
    assert_saturated_stop_finishes_once("progressing", Some(1_100));
}

fn assert_saturated_stop_finishes_once(phase: &str, progressing_at: Option<u64>) {
    let maximum = u64::try_from(i64::MAX).expect("maximum revision");
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        app.receiver.record_intent(true);
        let db = Db::open(app.context.workspace()).expect("state DB");
        let accepted = accept_email_job(&app, &db, "saturated terminal lifecycle", 100);
        let transport = TransportRecording::default();
        app.brain.replace_receiver_transport(transport.transport());
        app.tick_receiver();
        let session = rotate_active_session(
            &app,
            &format!("saturated-{phase}-{}-session", kind.as_str()),
        );
        let active = app.receiver.active_durable_run().expect("active receiver");
        let observation_path = active_observation_path(&app);
        let completion_path = active_completion_path(&app);
        let token = active.claim.job().token().to_string();
        write_saturated_snapshot(
            &observation_path,
            phase,
            progressing_at,
            &token,
            active.attribution.instance(),
            &session,
        );

        app.tick_receiver();

        let nonterminal = db.receiver_job(accepted.job_id()).unwrap().unwrap();
        assert_eq!(nonterminal.observation_revision(), maximum, "{kind:?}");
        assert_eq!(nonterminal.accepted_at_unix_ms(), Some(1_000), "{kind:?}");
        assert_eq!(
            nonterminal.progressing_at_unix_ms(),
            progressing_at,
            "{kind:?}"
        );
        let snapshot_before = std::fs::read(&observation_path).expect("saturated snapshot");

        produce_completion(&app, kind, &session, &observation_path);

        assert_eq!(
            std::fs::read(&observation_path).expect("settled saturated snapshot"),
            snapshot_before,
            "{kind:?} advanced the saturated revision"
        );
        assert!(completion_path.exists(), "{kind:?} withheld the artifact");
        assert_eq!(
            completion_status(&app, &session),
            "completed",
            "{kind:?} withheld the completed session"
        );

        app.tick_receiver();

        let completed = db.receiver_job(accepted.job_id()).unwrap().unwrap();
        assert_eq!(completed.state(), ReceiverJobState::AnswerReady, "{kind:?}");
        assert_eq!(completed.observation_revision(), maximum, "{kind:?}");
        assert_eq!(completed.accepted_at_unix_ms(), Some(1_000), "{kind:?}");
        assert_eq!(
            completed.progressing_at_unix_ms(),
            progressing_at,
            "{kind:?}"
        );
        let completed_at = completed
            .completed_at_unix_ms()
            .expect("artifact-only completion time");
        assert!(
            completed_at >= progressing_at.unwrap_or(1_000),
            "{kind:?} regressed the terminal timeline"
        );
        assert!(
            AgentObservationCursor::from_durable(
                maximum,
                Some(1_000),
                progressing_at,
                progressing_at,
                Some(completed_at),
            )
            .is_ok(),
            "{kind:?} stored an unrepresentable maximum cursor"
        );
        assert!(
            !completion_path.exists(),
            "{kind:?} retained the delivered artifact"
        );
        assert!(
            !observation_path.exists(),
            "{kind:?} retained the terminal snapshot"
        );
        assert_eq!(transport.shutdowns(), 1, "{kind:?}");

        app.tick_receiver();

        let delivery_started = db.receiver_job(accepted.job_id()).unwrap().unwrap();
        assert!(
            delivery_started.state() == ReceiverJobState::Delivering
                && delivery_started.observation_revision() == completed.observation_revision()
                && delivery_started.accepted_at_unix_ms() == completed.accepted_at_unix_ms()
                && delivery_started.progressing_at_unix_ms() == completed.progressing_at_unix_ms()
                && delivery_started.completed_at_unix_ms() == completed.completed_at_unix_ms(),
            "provider delivery changed saturated completion evidence"
        );
        assert_eq!(transport.shutdowns(), 1, "{kind:?} delivered twice");
    }
}

fn write_saturated_snapshot(
    path: &std::path::Path,
    phase: &str,
    progressing_at: Option<u64>,
    token: &str,
    instance: &str,
    session: &AgentSession,
) {
    std::fs::create_dir_all(path.parent().expect("observation parent"))
        .expect("observation directory");
    std::fs::write(
        path,
        serde_json::json!({
            "version": 1,
            "revision": i64::MAX,
            "phase": phase,
            "job_token": token,
            "instance_id": instance,
            "session_id": session.as_str(),
            "turn_id": progressing_at.map(|_| "saturated-turn"),
            "accepted_at_unix_ms": 1_000,
            "progressing_at_unix_ms": progressing_at,
            "latest_progress_at_unix_ms": progressing_at,
            "completed_at_unix_ms": null,
        })
        .to_string(),
    )
    .expect("saturated observation snapshot");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only saturated snapshot");
    }
}

fn completion_status(app: &App, session: &AgentSession) -> String {
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("completion status connection")
        .query_row(
            "SELECT completion_status FROM brain_sessions
             WHERE agent_session_id = ?1",
            [session.as_str()],
            |row| row.get(0),
        )
        .expect("completion status")
}
