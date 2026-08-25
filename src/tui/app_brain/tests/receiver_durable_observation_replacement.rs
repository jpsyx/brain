use super::receiver_durable_support::accept_email_job;
use super::*;

use crate::state::ReceiverJobState;

#[cfg(unix)]
#[test]
fn replaced_observation_files_never_advance_the_durable_job() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    for replacement in [
        "symlink",
        "permissive",
        "wrong-token",
        "truncated",
        "lower-revision",
        "impossible-revision",
    ] {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&temporary, &cli, AgentKind::Claude);
        app.receiver.record_intent(true);
        let db = Db::open(app.context.workspace()).expect("state DB");
        let accepted = accept_email_job(&app, &db, "synthetic replacement", 100);
        let transport = TransportRecording::default();
        app.brain.replace_receiver_transport(transport.transport());
        app.tick_receiver();
        let session = rotate_active_session(&app, "native-replacement");
        let path = active_observation_path(&app);

        if replacement == "lower-revision" {
            write_owner_only(&path, valid_snapshot(&app, &session, 2).to_string());
            app.tick_receiver();
            let seeded = db.receiver_job(accepted.job_id()).unwrap().unwrap();
            assert_eq!(seeded.state(), ReceiverJobState::Accepted);
            assert_eq!(seeded.observation_revision(), 2);
        }
        let before = db.receiver_job(accepted.job_id()).unwrap().unwrap();
        let mut snapshot = valid_snapshot(&app, &session, 1);
        match replacement {
            "symlink" => {
                let outside = temporary.path().join("outside.json");
                write_owner_only(&outside, snapshot.to_string());
                std::fs::create_dir_all(path.parent().expect("observation parent"))
                    .expect("observation directory");
                let _ = std::fs::remove_file(&path);
                symlink(&outside, &path).expect("observation symlink");
            }
            "permissive" => {
                write_owner_only(&path, snapshot.to_string());
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                    .expect("permissive observation");
            }
            "wrong-token" => {
                snapshot["job_token"] = serde_json::json!("22222222-2222-4222-8222-222222222222");
                write_owner_only(&path, snapshot.to_string());
            }
            "truncated" => write_owner_only(&path, b"{\"version\":1"),
            "lower-revision" => write_owner_only(&path, snapshot.to_string()),
            "impossible-revision" => {
                snapshot["revision"] =
                    serde_json::json!(u64::try_from(i64::MAX).expect("i64 maximum fits u64") + 1);
                write_owner_only(&path, snapshot.to_string());
            }
            _ => unreachable!("complete replacement table"),
        }

        app.tick_receiver();

        assert_eq!(
            db.receiver_job(accepted.job_id()).unwrap().unwrap(),
            before,
            "{replacement} changed durable lifecycle facts"
        );
        assert_eq!(
            app.brain.receiver_run_observations().len(),
            1,
            "{replacement} removed the active receiver tab"
        );
        assert_eq!(transport.shutdowns(), 0, "{replacement}");
    }
}

#[cfg(unix)]
fn write_owner_only(path: &std::path::Path, body: impl AsRef<[u8]>) {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::create_dir_all(path.parent().expect("observation parent"))
        .expect("observation directory");
    std::fs::write(path, body).expect("observation snapshot");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .expect("owner-only observation");
}

#[cfg(unix)]
fn valid_snapshot(app: &App, session: &AgentSession, revision: u64) -> serde_json::Value {
    let active = app.receiver.active_durable_run().expect("active receiver");
    serde_json::json!({
        "version": 1,
        "revision": revision,
        "phase": "accepted",
        "job_token": active.claim.job().token().to_string(),
        "instance_id": active.attribution.instance(),
        "session_id": session.as_str(),
        "turn_id": null,
        "accepted_at_unix_ms": 1_000,
        "progressing_at_unix_ms": null,
        "completed_at_unix_ms": null,
    })
}

#[cfg(unix)]
fn active_observation_path(app: &App) -> std::path::PathBuf {
    let instance = app
        .receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .instance();
    app.context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"))
}

#[cfg(unix)]
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
