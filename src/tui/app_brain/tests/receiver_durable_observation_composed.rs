use super::receiver_durable_support::{accept_email_job, mark_receiver_session_completed};
use super::*;

use crate::state::{ReceiverJobState, ReceiverNonterminalObservationPhase, ReceiverObservation};

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
                phase: ReceiverNonterminalObservationPhase::Accepted,
                revision: 1,
                observed_at_unix_ms: 1_000,
                authorized_at_unix_ms: 1_050,
            },
        )
        .expect("seed durable accepted evidence")
    );
    let state_path = app.context.state_db_path().to_path_buf();
    let (normalized_boundaries, observed_boundaries) = std::sync::mpsc::sync_channel(1);
    app.receiver
        .install_before_observation_persistence_hook(Box::new(move |boundaries| {
            normalized_boundaries
                .send(
                    boundaries
                        .iter()
                        .map(|boundary| boundary.phase())
                        .collect::<Vec<_>>(),
                )
                .expect("record normalized boundaries");
        }));
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
    mark_receiver_session_completed(&app, &native);

    app.tick_receiver();

    assert_eq!(
        observed_boundaries.recv().expect("normalized boundaries"),
        [
            crate::agent::AgentObservationPhase::Progressing,
            crate::agent::AgentObservationPhase::Completed,
        ],
        "the durable accepted cursor must suppress accepted before persistence"
    );
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

#[test]
fn exact_maximum_revision_roundtrips_without_wrap_or_false_newer_evidence() {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Codex);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "maximum revision", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let session = rotate_active_session(&app, "maximum-revision-session");
    let active = app.receiver.active_durable_run().expect("active receiver");
    let instance = active.attribution.instance().to_owned();
    let token = active.claim.job().token().to_string();
    let path = app
        .context
        .workspace()
        .paths()
        .receiver_observations_dir()
        .join(format!("{instance}.json"));
    write_owner_only_snapshot(
        &path,
        &serde_json::json!({
            "version": 1,
            "revision": i64::MAX,
            "phase": "progressing",
            "job_token": token,
            "instance_id": instance,
            "session_id": session.as_str(),
            "turn_id": "maximum-turn",
            "accepted_at_unix_ms": 1_000,
            "progressing_at_unix_ms": 1_100,
            "completed_at_unix_ms": null,
        }),
    );

    app.tick_receiver();

    let durable = db.receiver_job(accepted.job_id()).unwrap().unwrap();
    let maximum = u64::try_from(i64::MAX).expect("maximum is nonnegative");
    assert_eq!(durable.state(), ReceiverJobState::Processing);
    assert_eq!(durable.observation_revision(), maximum);
    assert_eq!(durable.accepted_at_unix_ms(), Some(1_000));
    assert_eq!(durable.progressing_at_unix_ms(), Some(1_100));
    assert_eq!(durable.completed_at_unix_ms(), None);
    let stored_revision = rusqlite::Connection::open(app.context.state_db_path())
        .expect("state connection")
        .query_row(
            "SELECT observation_revision FROM receiver_jobs WHERE job_id = ?1",
            [accepted.job_id().to_string()],
            |row| row.get::<_, i64>(0),
        )
        .expect("stored revision");
    assert_eq!(stored_revision, i64::MAX);
    assert!(stored_revision >= 0);
    assert_eq!(
        app.services
            .receiver_observation_cursor(accepted.job_id())
            .expect("durable cursor"),
        Some((
            ReceiverJobState::Processing,
            crate::agent::AgentObservationCursor::from_durable(
                maximum,
                Some(1_000),
                Some(1_100),
                None,
            )
            .expect("maximum durable cursor"),
        ))
    );

    let snapshot_before = std::fs::read(&path).expect("maximum snapshot");
    let payload = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "thread_id": session.as_str(),
        "turn_id": "must-not-wrap",
    });
    let mut child = Command::new("python3")
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts/receiver_observation_bridge.py"),
        )
        .env("BRAIN_AGENT_KIND", "codex")
        .env("BRAIN_INSTANCE_ID", &instance)
        .env("BRAIN_RECEIVER_JOB_TOKEN", &token)
        .env("BRAIN_RECEIVER_OBSERVATION_PATH", &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn saturated producer");
    child
        .stdin
        .take()
        .expect("producer stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write saturated payload");
    let output = child.wait_with_output().expect("wait saturated producer");
    assert!(
        output.status.success(),
        "saturated producer failed: {output:?}"
    );
    assert_eq!(
        std::fs::read(&path).expect("snapshot after saturated producer"),
        snapshot_before
    );

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap(),
        durable
    );
    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert_eq!(transport.shutdowns(), 0);
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

fn write_owner_only_snapshot(path: &std::path::Path, snapshot: &serde_json::Value) {
    std::fs::create_dir_all(path.parent().expect("observation parent"))
        .expect("observation directory");
    std::fs::write(path, snapshot.to_string()).expect("observation snapshot");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only observation");
    }
}
