use super::receiver_durable_support::{
    accept_email_job, mark_receiver_session_completed, publish_valid_rotated_completion,
};
use super::*;

use crate::state::ReceiverJobState;

#[test]
fn every_frontend_persists_only_exact_new_lifecycle_evidence() {
    for kind in AgentKind::ALL {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, kind);
        app.receiver.record_intent(true);
        let db = Db::open(app.context.workspace()).expect("state DB");
        let accepted = accept_email_job(&app, &db, "observe exact receiver prompt", 100);
        app.brain
            .replace_receiver_transport(TransportRecording::default().transport());

        app.tick_receiver();
        app.tick_receiver();

        let launched = db.receiver_job(accepted.job_id()).unwrap().unwrap();
        assert_eq!(launched.state(), ReceiverJobState::Launched, "{kind:?}");
        assert_eq!(launched.observation_revision(), 0, "{kind:?}");
        assert_eq!(launched.accepted_at_unix_ms(), None, "{kind:?}");

        let session = rotate_active_session(&app, &format!("native-{kind:?}"));
        write_active_snapshot(&app, &session, 1, "accepted", Some(1_000), None, None);
        app.tick_receiver();

        let accepted_job = db.receiver_job(accepted.job_id()).unwrap().unwrap();
        assert_eq!(accepted_job.state(), ReceiverJobState::Accepted, "{kind:?}");
        assert_eq!(accepted_job.observation_revision(), 1, "{kind:?}");
        assert_eq!(accepted_job.accepted_at_unix_ms(), Some(1_000), "{kind:?}");
        assert_eq!(accepted_job.progressing_at_unix_ms(), None, "{kind:?}");

        let path = write_active_snapshot(
            &app,
            &session,
            2,
            "progressing",
            Some(1_000),
            Some(1_100),
            None,
        );
        app.tick_receiver();

        let progressing = db.receiver_job(accepted.job_id()).unwrap().unwrap();
        assert_eq!(
            progressing.state(),
            ReceiverJobState::Processing,
            "{kind:?}"
        );
        assert_eq!(progressing.observation_revision(), 2, "{kind:?}");
        assert_eq!(progressing.accepted_at_unix_ms(), Some(1_000), "{kind:?}");
        assert_eq!(
            progressing.progressing_at_unix_ms(),
            Some(1_100),
            "{kind:?}"
        );
        assert_eq!(
            progressing.latest_progress_at_unix_ms(),
            Some(1_100),
            "{kind:?}"
        );

        let mut pulse: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&path).expect("progressing observation snapshot"),
        )
        .expect("progressing observation JSON");
        pulse["revision"] = serde_json::json!(3);
        pulse["turn_id"] = serde_json::json!("later-progress");
        pulse["latest_progress_at_unix_ms"] = serde_json::json!(1_200);
        write_owner_only(&path, pulse.to_string());
        app.tick_receiver();

        let pulsed = db.receiver_job(accepted.job_id()).unwrap().unwrap();
        assert_eq!(pulsed.observation_revision(), 3, "{kind:?}");
        assert_eq!(pulsed.progressing_at_unix_ms(), Some(1_100), "{kind:?}");
        assert_eq!(pulsed.latest_progress_at_unix_ms(), Some(1_200), "{kind:?}");
        assert_eq!(app.brain.receiver_run_observations().len(), 1, "{kind:?}");
    }
}

#[test]
fn malformed_unrelated_and_equal_revision_evidence_never_changes_durable_state() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "delayed evidence", 100);
    app.brain
        .replace_receiver_transport(TransportRecording::default().transport());
    app.tick_receiver();
    let session = rotate_active_session(&app, "native-delayed");
    let path = write_active_snapshot(&app, &session, 1, "accepted", Some(1_000), None, None);
    let mut wrong = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(&path).expect("snapshot body"),
    )
    .expect("snapshot JSON");
    wrong["job_token"] = serde_json::json!("00000000-0000-4000-8000-000000000002");
    write_owner_only(&path, wrong.to_string());

    app.tick_receiver();
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launched
    );

    write_owner_only(&path, "not-json");
    app.tick_receiver();
    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launched
    );

    write_active_snapshot(&app, &session, 1, "accepted", Some(1_000), None, None);
    app.tick_receiver();
    write_active_snapshot(
        &app,
        &session,
        1,
        "progressing",
        Some(1_000),
        Some(1_100),
        None,
    );
    app.tick_receiver();

    let durable = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(durable.state(), ReceiverJobState::Accepted);
    assert_eq!(durable.observation_revision(), 1);
    assert_eq!(durable.progressing_at_unix_ms(), None);
}

#[test]
fn completion_only_evidence_finishes_without_a_response_and_unblocks_fifo() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Codex);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let first = accept_email_job(&app, &db, "completion first", 100);
    let second = accept_email_job(&app, &db, "wait behind first", 200);
    let first_transport = TransportRecording::default();
    app.brain
        .replace_receiver_transport(first_transport.transport());
    app.tick_receiver();
    let session = rotate_active_session(&app, "native-completion-only");
    write_active_snapshot(
        &app,
        &session,
        3,
        "completed",
        Some(1_000),
        Some(1_100),
        Some(1_200),
    );
    mark_receiver_session_completed(&app, &session);

    app.tick_receiver();

    let completed = db.receiver_job(first.job_id()).unwrap().unwrap();
    assert_eq!(completed.state(), ReceiverJobState::Done);
    assert_eq!(completed.accepted_at_unix_ms(), Some(1_000));
    assert_eq!(completed.progressing_at_unix_ms(), Some(1_100));
    assert_eq!(completed.completed_at_unix_ms(), Some(1_200));
    assert_eq!(completed.observation_revision(), 3);
    assert_eq!(completed.observation_session_id(), Some(session.as_str()));
    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(first_transport.shutdowns(), 1);
    assert_eq!(
        db.receiver_job(second.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Queued
    );

    app.brain
        .replace_receiver_transport(TransportRecording::default().transport());
    app.tick_receiver();
    assert_eq!(
        app.brain.receiver_run_observations()[0].job_id,
        second.job_id()
    );
}

#[test]
fn artifact_and_lifecycle_completion_in_one_tick_finish_once_through_artifact_delivery() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "one terminal boundary", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let response =
        publish_valid_rotated_completion(&app, "native-artifact-and-lifecycle", "exact response");
    let session = AgentSession::new("native-artifact-and-lifecycle").unwrap();
    write_active_snapshot(
        &app,
        &session,
        3,
        "completed",
        Some(1_000),
        Some(1_100),
        Some(1_200),
    );

    app.tick_receiver();

    let completed = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(completed.state(), ReceiverJobState::Done);
    assert_eq!(completed.accepted_at_unix_ms(), Some(1_000));
    assert_eq!(completed.progressing_at_unix_ms(), Some(1_100));
    assert_eq!(completed.completed_at_unix_ms(), Some(1_200));
    assert_eq!(completed.observation_revision(), 3);
    assert_eq!(completed.observation_session_id(), Some(session.as_str()));
    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
    assert!(!response.exists());
}

#[test]
fn owner_loss_between_observation_and_commit_preserves_evidence_and_cleans_locally() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::OpenCode);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "ownership race", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let session = rotate_active_session(&app, "native-owner-race");
    write_active_snapshot(&app, &session, 1, "accepted", Some(1_000), None, None);
    let state_db = app.context.state_db_path().to_owned();
    let job_id = accepted.job_id();
    app.receiver
        .install_after_observation_validation_hook(Box::new(move || {
            rusqlite::Connection::open(&state_db)
                .expect("racing state DB")
                .execute(
                    "UPDATE receiver_jobs SET claim_owner = 'replacement-owner' WHERE job_id = ?1",
                    [job_id.to_string()],
                )
                .expect("replace receiver owner");
        }));

    app.tick_receiver();

    let durable = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(durable.state(), ReceiverJobState::Launched);
    assert_eq!(durable.accepted_at_unix_ms(), None);
    assert_eq!(durable.observation_revision(), 0);
    assert!(app.brain.receiver_run_observations().is_empty());
    assert_eq!(transport.shutdowns(), 1);
}

#[test]
fn session_unlock_between_observation_and_commit_rejects_stored_session_continuity() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "session ownership race", 100);
    app.brain
        .replace_receiver_transport(TransportRecording::default().transport());
    app.tick_receiver();
    let session = rotate_active_session(&app, "native-session-race");
    write_active_snapshot(&app, &session, 1, "accepted", Some(1_000), None, None);
    app.tick_receiver();
    write_active_snapshot(
        &app,
        &session,
        2,
        "progressing",
        Some(1_000),
        Some(1_100),
        None,
    );
    let state_db = app.context.state_db_path().to_owned();
    let instance = app
        .receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .instance()
        .to_owned();
    app.receiver
        .install_after_observation_validation_hook(Box::new(move || {
            rusqlite::Connection::open(&state_db)
                .expect("racing state DB")
                .execute(
                    "UPDATE brain_sessions SET locked_pid = NULL
                     WHERE brain_instance_id = ?1",
                    [instance],
                )
                .expect("release exact native session");
        }));

    app.tick_receiver();

    let durable = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    assert_eq!(durable.state(), ReceiverJobState::Accepted);
    assert_eq!(durable.observation_revision(), 1);
    assert_eq!(durable.progressing_at_unix_ms(), None);
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

fn write_active_snapshot(
    app: &App,
    session: &AgentSession,
    revision: u64,
    phase: &str,
    accepted_at_unix_ms: Option<u64>,
    progressing_at_unix_ms: Option<u64>,
    completed_at_unix_ms: Option<u64>,
) -> std::path::PathBuf {
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
            "revision": revision,
            "phase": phase,
            "job_token": active.claim.job().token().to_string(),
            "instance_id": instance,
            "session_id": session.as_str(),
            "turn_id": null,
            "accepted_at_unix_ms": accepted_at_unix_ms,
            "progressing_at_unix_ms": progressing_at_unix_ms,
            "latest_progress_at_unix_ms": progressing_at_unix_ms,
            "completed_at_unix_ms": completed_at_unix_ms,
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
    path
}

fn write_owner_only(path: &std::path::Path, body: impl AsRef<[u8]>) {
    std::fs::write(path, body).expect("observation snapshot");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only observation");
    }
}
