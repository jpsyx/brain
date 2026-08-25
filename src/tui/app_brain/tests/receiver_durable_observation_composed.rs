use super::receiver_durable_support::accept_email_job;
use super::*;

use crate::state::{ReceiverJobState, ReceiverObservation, ReceiverObservationPhase};

#[test]
fn one_app_poll_rebuilds_the_durable_cursor_and_commits_only_missed_boundaries_atomically() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "missed lifecycle boundaries", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let active = app.receiver.active_durable_run().expect("active receiver");
    let job_id = active.claim.job().id();
    assert_eq!(job_id, accepted.job_id());
    let token = active.claim.job().token();
    let owner = active.claim.claim().owner().to_owned();
    let instance = active.attribution.instance().to_owned();
    let conversation_id = active.claim.job().conversation_id();
    let native = rotate_active_session(&app, "session-1");
    assert!(
        db.apply_receiver_observation(
            job_id,
            &owner,
            &ReceiverObservation {
                token,
                instance,
                session_id: native.as_str().to_owned(),
                phase: ReceiverObservationPhase::Accepted,
                revision: 1,
                observed_at_unix_ms: 1_000,
                authorized_at_unix_ms: 1_050,
            },
        )
        .expect("seed durable accepted evidence")
    );
    let state_path = app.context.state_db_path().to_path_buf();
    let (before_tx, observed_before_tx) = std::sync::mpsc::sync_channel(1);
    app.receiver
        .install_after_observation_validation_hook(Box::new(move || {
            let connection = rusqlite::Connection::open(&state_path).expect("pre-transaction DB");
            let evidence = connection
                .query_row(
                    "SELECT state, accepted_at_unix_ms, progressing_at_unix_ms,
                            completed_at_unix_ms, observation_revision
                     FROM receiver_jobs WHERE job_id = ?1",
                    [job_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<i64>>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                            row.get::<_, Option<i64>>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    },
                )
                .expect("pre-transaction job");
            before_tx
                .send(evidence)
                .expect("record pre-transaction durable evidence");
        }));
    write_snapshot_with_missed_boundaries(&app, &native);

    app.tick_receiver();

    assert_eq!(
        observed_before_tx.recv().expect("pre-transaction evidence"),
        ("accepted".to_owned(), Some(1_000), None, None, 1),
        "the App must rebuild its cursor from durable accepted evidence before one atomic write"
    );
    let completed = db.receiver_job(job_id).unwrap().unwrap();
    assert_eq!(completed.state(), ReceiverJobState::Done);
    assert_eq!(completed.accepted_at_unix_ms(), Some(1_000));
    assert_eq!(completed.progressing_at_unix_ms(), Some(1_100));
    assert_eq!(completed.completed_at_unix_ms(), Some(1_200));
    assert_eq!(completed.observation_revision(), 3);
    assert_eq!(
        db.receiver_conversation(conversation_id)
            .unwrap()
            .unwrap()
            .binding()
            .map(crate::state::ReceiverSessionBinding::native_session_id),
        Some(native.as_str())
    );
    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
}

fn rotate_active_session(app: &App, session_id: &str) -> AgentSession {
    let active = app.receiver.active_durable_run().expect("active receiver");
    let session = AgentSession::new(session_id).expect("native session");
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("lifecycle fixture connection")
        .execute(
            "UPDATE brain_sessions SET agent_session_id = ?1 WHERE brain_instance_id = ?2",
            rusqlite::params![session.as_str(), active.attribution.instance()],
        )
        .expect("simulate lifecycle native session");
    session
}

fn write_snapshot_with_missed_boundaries(app: &App, session: &AgentSession) {
    let active = app.receiver.active_durable_run().expect("active receiver");
    let instance = active.attribution.instance();
    let path = app
        .context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"));
    std::fs::create_dir_all(path.parent().expect("observation parent"))
        .expect("observation directory");
    std::fs::write(
        &path,
        serde_json::json!({
            "version": 1,
            "revision": 3,
            "phase": "completed",
            "job_token": active.claim.job().token().to_string(),
            "instance_id": instance,
            "session_id": session.as_str(),
            "turn_id": null,
            "accepted_at_unix_ms": 1_000,
            "progressing_at_unix_ms": 1_100,
            "completed_at_unix_ms": 1_200,
        })
        .to_string(),
    )
    .expect("observation snapshot");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only observation");
    }
}
