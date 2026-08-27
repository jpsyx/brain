use super::receiver_durable_support::{
    accept_email_job, mark_receiver_session_completed, publish_valid_rotated_completion,
};
use super::*;

use crate::state::ReceiverJobState;

#[test]
fn artifact_completion_removes_only_the_exact_instance_files() {
    let (_temporary, mut app, db, accepted, transport) = launched_receiver(AgentKind::Claude);
    publish_valid_rotated_completion(&app, "native-artifact-cleanup", "artifact response");
    let files = ReceiverInstanceFiles::seed(&app, true);

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done
    );
    files.assert_exact_removed_and_unrelated_retained();
    assert_eq!(transport.shutdowns(), 1);
}

#[test]
fn lifecycle_completion_removes_only_the_exact_instance_files() {
    let (_temporary, mut app, db, accepted, transport) = launched_receiver(AgentKind::Codex);
    let session = rotate_active_session(&app, uuid::Uuid::new_v4().to_string());
    let files = ReceiverInstanceFiles::seed(&app, false);
    write_completed_snapshot(&app, &session);
    mark_receiver_session_completed(&app, &session);

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done
    );
    files.assert_exact_removed_and_unrelated_retained();
    app.tick_receiver();
    files.assert_exact_removed_and_unrelated_retained();
    assert_eq!(transport.shutdowns(), 1);
}

#[test]
fn child_exit_removes_only_the_exact_instance_files() {
    let (_temporary, mut app, db, accepted, transport) = launched_receiver(AgentKind::OpenCode);
    let durable_before = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    let files = ReceiverInstanceFiles::seed(&app, false);
    transport.set_alive(false);

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap(),
        durable_before
    );
    files.assert_exact_removed_and_unrelated_retained();
    assert_eq!(transport.shutdowns(), 1);
}

#[test]
fn lost_ownership_removes_only_the_exact_instance_files() {
    let (_temporary, mut app, db, accepted, transport) = launched_receiver(AgentKind::Claude);
    let files = ReceiverInstanceFiles::seed(&app, false);
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("ownership fixture connection")
        .execute(
            "UPDATE receiver_jobs SET claim_owner = 'replacement-owner' WHERE job_id = ?1",
            [accepted.job_id().to_string()],
        )
        .expect("replace receiver owner");

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launched
    );
    files.assert_exact_removed_and_unrelated_retained();
    assert_eq!(transport.shutdowns(), 1);
}

#[test]
fn orderly_shutdown_removes_only_the_exact_instance_files() {
    let (_temporary, mut app, db, accepted, transport) = launched_receiver(AgentKind::Codex);
    let durable_before = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    let files = ReceiverInstanceFiles::seed(&app, false);

    app.shutdown_receiver_runtime();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap(),
        durable_before
    );
    files.assert_exact_removed_and_unrelated_retained();
    assert_eq!(transport.shutdowns(), 1);
}

fn launched_receiver(
    kind: AgentKind,
) -> (
    tempfile::TempDir,
    App,
    Db,
    crate::state::ReceiverAcceptance,
    TransportRecording,
) {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, kind);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "exact cleanup", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    (temporary, app, db, accepted, transport)
}

struct ReceiverInstanceFiles {
    exact: [PathBuf; 3],
    unrelated: [PathBuf; 3],
}

impl ReceiverInstanceFiles {
    fn seed(app: &App, response_already_exists: bool) -> Self {
        let instance = app
            .receiver
            .active_durable_run()
            .expect("active receiver")
            .attribution
            .instance();
        let exact = paths_for(app, instance);
        let unrelated = paths_for(app, "unrelated-receiver-instance");
        std::fs::create_dir_all(exact[0].parent().expect("response directory"))
            .expect("response directory");
        std::fs::create_dir_all(exact[1].parent().expect("observation directory"))
            .expect("observation directory");
        if !response_already_exists {
            std::fs::write(&exact[0], "partial response").expect("exact response");
        }
        std::fs::write(&exact[1], "partial observation").expect("exact observation");
        std::fs::write(&exact[2], "exact lock").expect("exact lock");
        for (path, body) in unrelated.iter().zip([
            "unrelated response",
            "unrelated observation",
            "unrelated lock",
        ]) {
            std::fs::write(path, body).expect("unrelated instance file");
        }
        Self { exact, unrelated }
    }

    fn assert_exact_removed_and_unrelated_retained(&self) {
        for path in &self.exact {
            assert!(!path.exists(), "exact receiver file remains: {path:?}");
        }
        for path in &self.unrelated {
            assert!(
                path.exists(),
                "unrelated receiver file was removed: {path:?}"
            );
        }
    }
}

fn paths_for(app: &App, instance: &str) -> [PathBuf; 3] {
    let response = app
        .context
        .workspace()
        .paths()
        .responses_dir()
        .join(format!("{instance}.json"));
    let observation = app
        .context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"));
    let lock = observation.with_extension("json.lock");
    [response, observation, lock]
}

fn rotate_active_session(app: &App, session_id: impl Into<String>) -> AgentSession {
    let active = app.receiver.active_durable_run().expect("active receiver");
    let session = AgentSession::new(session_id.into()).expect("native session");
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("lifecycle fixture connection")
        .execute(
            "UPDATE brain_sessions SET agent_session_id = ?1 WHERE brain_instance_id = ?2",
            rusqlite::params![session.as_str(), active.attribution.instance()],
        )
        .expect("simulate lifecycle native session");
    session
}

fn write_completed_snapshot(app: &App, session: &AgentSession) {
    let active = app.receiver.active_durable_run().expect("active receiver");
    let instance = active.attribution.instance();
    let path = app
        .context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"));
    std::fs::write(
        &path,
        serde_json::json!({
            "version": 1,
            "revision": 1,
            "phase": "completed",
            "job_token": active.claim.job().token().to_string(),
            "instance_id": instance,
            "session_id": session.as_str(),
            "turn_id": null,
            "accepted_at_unix_ms": null,
            "progressing_at_unix_ms": null,
            "latest_progress_at_unix_ms": null,
            "completed_at_unix_ms": 1_200,
        })
        .to_string(),
    )
    .expect("completed observation snapshot");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only observation");
    }
}
