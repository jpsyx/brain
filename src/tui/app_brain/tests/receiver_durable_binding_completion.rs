use super::receiver_durable_support::{
    ReceiverClock, accept_email_job, publish_valid_completion, publish_valid_rotated_completion,
};
use super::*;

use crate::state::ReceiverJobState;

#[test]
fn completion_validated_after_claim_expiry_cannot_finalize_or_run_terminal_effects() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "complete at the lease boundary", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let attribution = app
        .receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .clone();
    let completion_path = publish_valid_completion(&app, "stale owner response");
    let before = (
        app.shell.main_view(),
        app.effective_brain_tab(),
        app.shell.focus(),
    );
    app.receiver
        .install_after_completion_validation_hook(Box::new(move || {
            clock.advance(std::time::Duration::from_secs(31));
        }));

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launched,
        "an observation older than the lease must not authorize terminal commit"
    );
    assert!(
        db.receiver_conversation(accepted.conversation_id())
            .unwrap()
            .unwrap()
            .binding()
            .is_none(),
        "the stale owner must not persist its native binding"
    );
    assert!(completion_path.exists());
    assert_eq!(transport.shutdowns(), 0);
    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert!(
        app.services
            .locked_session_for_instance(attribution.instance(), attribution.scope())
            .is_some()
    );
    assert_eq!(
        (
            app.shell.main_view(),
            app.effective_brain_tab(),
            app.shell.focus(),
        ),
        before
    );
}

#[test]
fn completion_validated_before_owner_replacement_cannot_finalize_for_the_old_owner() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let clock = ReceiverClock::new();
    app.services
        .replace_receiver_sync_runtime(Box::new(clock.clone()));
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "complete across owner replacement", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let completion_path = publish_valid_completion(&app, "old owner response");
    let workspace = Arc::clone(&app.context.command().workspace);
    app.receiver
        .install_after_completion_validation_hook(Box::new(move || {
            clock.advance(std::time::Duration::from_secs(31));
            let now = clock.unix_ms();
            Db::open(&workspace)
                .expect("replacement state DB")
                .claim_next_receiver_run("replacement-owner", now, now + 30_000)
                .expect("replacement claim")
                .expect("expired launching run is reclaimable");
        }));

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Retrying,
        "the replacement owner may recover the expired launch, but the old owner cannot finish it"
    );
    assert!(
        db.receiver_conversation(accepted.conversation_id())
            .unwrap()
            .unwrap()
            .binding()
            .is_none()
    );
    assert!(completion_path.exists());
    assert_eq!(transport.shutdowns(), 0);
}

#[test]
fn native_binding_mismatch_keeps_exact_completion_retryable() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Codex);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "complete after native rotation", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let attribution = app
        .receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .clone();
    let completion_path = publish_valid_completion(&app, "completed before binding rotated");

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launched,
        "an unproved native binding must not make completion irreversible"
    );
    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert_eq!(transport.shutdowns(), 0);
    assert!(completion_path.exists());
    assert!(
        db.receiver_conversation(accepted.conversation_id())
            .unwrap()
            .unwrap()
            .binding()
            .is_none()
    );
    assert!(
        app.services
            .locked_session_for_instance(attribution.instance(), attribution.scope())
            .is_some()
    );

    let native_session = AgentSession::new("rotated-codex-native").expect("native session");
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("lifecycle connection")
        .execute(
            "UPDATE brain_sessions SET agent_session_id = ?1 WHERE brain_instance_id = ?2",
            rusqlite::params![native_session.as_str(), attribution.instance()],
        )
        .expect("simulate lifecycle native rotation");
    write_completion(&app, &attribution, &completion_path, &native_session);

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done
    );
    assert_eq!(transport.shutdowns(), 1);
    assert!(!completion_path.exists());
    assert_eq!(
        db.receiver_conversation(accepted.conversation_id())
            .unwrap()
            .unwrap()
            .binding()
            .map(crate::state::ReceiverSessionBinding::native_session_id),
        Some(native_session.as_str())
    );
}

#[test]
fn native_binding_write_error_keeps_exact_completion_retryable() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "complete after transient storage error", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let attribution = app
        .receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .clone();
    let completion_path = publish_valid_completion(&app, "completed before durable binding");
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("binding fault connection")
        .execute_batch(
            "CREATE TRIGGER fail_receiver_native_binding
             BEFORE UPDATE OF agent_session_id ON receiver_conversations
             BEGIN
               SELECT RAISE(FAIL, 'injected native binding failure');
             END;",
        )
        .expect("install deterministic binding failure");

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launched,
        "a transient binding write error must not make completion irreversible"
    );
    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert_eq!(transport.shutdowns(), 0);
    assert!(completion_path.exists());
    assert!(
        db.receiver_conversation(accepted.conversation_id())
            .unwrap()
            .unwrap()
            .binding()
            .is_none()
    );
    assert!(
        app.services
            .locked_session_for_instance(attribution.instance(), attribution.scope())
            .is_some()
    );

    rusqlite::Connection::open(app.context.state_db_path())
        .expect("binding repair connection")
        .execute("DROP TRIGGER fail_receiver_native_binding", [])
        .expect("remove deterministic binding failure");

    app.tick_receiver();

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Done
    );
    assert_eq!(transport.shutdowns(), 1);
    assert!(!completion_path.exists());
    assert!(
        db.receiver_conversation(accepted.conversation_id())
            .unwrap()
            .unwrap()
            .binding()
            .is_some()
    );
}

#[test]
fn lifecycle_rotation_after_validation_cannot_finalize_the_old_completion() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Codex);
    app.receiver.record_intent(true);
    let db = Db::open(app.context.workspace()).expect("state DB");
    let accepted = accept_email_job(&app, &db, "complete across lifecycle race", 100);
    let transport = TransportRecording::default();
    app.brain.replace_receiver_transport(transport.transport());
    app.tick_receiver();
    let attribution = app
        .receiver
        .active_durable_run()
        .expect("active receiver")
        .attribution
        .clone();
    let old_native = AgentSession::new("old-completed-native").expect("old native session");
    let completion_path =
        publish_valid_rotated_completion(&app, old_native.as_str(), "old completed response");
    let validation_reached = Arc::new(std::sync::Barrier::new(2));
    let lifecycle_validation_reached = Arc::clone(&validation_reached);
    let state_path = app.context.state_db_path().to_path_buf();
    let instance = attribution.instance().to_owned();
    let old_native_id = old_native.as_str().to_owned();
    let (rotation_result_tx, rotation_result_rx) = std::sync::mpsc::sync_channel(1);
    let lifecycle = std::thread::spawn(move || {
        lifecycle_validation_reached.wait();
        let result =
            rotate_receiver_lifecycle(&state_path, &instance, &old_native_id, "new-active-native")
                .map_err(|error| error.to_string());
        rotation_result_tx
            .send(result)
            .expect("publish lifecycle result");
    });
    app.receiver
        .install_after_completion_validation_hook(Box::new(move || {
            validation_reached.wait();
            rotation_result_rx
                .recv()
                .expect("lifecycle result")
                .expect("rotate lifecycle after validation");
        }));

    app.tick_receiver();
    lifecycle.join().expect("lifecycle thread");

    assert_eq!(
        db.receiver_job(accepted.job_id()).unwrap().unwrap().state(),
        ReceiverJobState::Launched,
        "a different lifecycle session must not finalize the validated artifact"
    );
    assert!(completion_path.exists());
    assert_eq!(transport.shutdowns(), 0);
    assert_eq!(app.brain.receiver_run_observations().len(), 1);
    assert!(
        db.receiver_conversation(accepted.conversation_id())
            .unwrap()
            .unwrap()
            .binding()
            .is_none(),
        "the new active session is not proof for the old completion artifact"
    );
}

fn rotate_receiver_lifecycle(
    state_path: &std::path::Path,
    instance: &str,
    old_native_id: &str,
    new_native_id: &str,
) -> rusqlite::Result<()> {
    let connection = rusqlite::Connection::open(state_path)?;
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let inserted = connection.execute(
        "INSERT INTO brain_sessions
           (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
            workspace_id, actor_id, channel, created_at, last_active_at, completion_status)
         SELECT agent_kind, ?1, brain_instance_id, locked_pid, 'startup',
                workspace_id, actor_id, channel, created_at, last_active_at, 'active'
         FROM brain_sessions
         WHERE brain_instance_id = ?2 AND agent_session_id = ?3
           AND locked_pid IS NOT NULL",
        rusqlite::params![new_native_id, instance, old_native_id],
    )?;
    if inserted != 1 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    connection.execute(
        "UPDATE brain_sessions SET locked_pid = NULL
         WHERE brain_instance_id = ?1 AND agent_session_id = ?2",
        rusqlite::params![instance, old_native_id],
    )?;
    connection.execute_batch("COMMIT")?;
    Ok(())
}

fn write_completion(
    app: &App,
    attribution: &crate::state::ReceiverSessionAttribution,
    path: &std::path::Path,
    native_session: &AgentSession,
) {
    std::fs::write(
        path,
        serde_json::json!({
            "session_id": native_session.as_str(),
            "response_id": attribution.instance(),
            "frontend": attribution.scope().agent_kind().as_str(),
            "workspace_id": app.context.workspace().id().to_string(),
            "actor_id": attribution.scope().actor().user_id().as_str(),
            "channel": attribution.scope().actor().channel().as_str(),
            "completion_status": "completed",
            "job_token": app
                .receiver
                .active_durable_run()
                .expect("active receiver run")
                .claim
                .job()
                .token()
                .to_string(),
            "message": "completed after durable binding repair",
        })
        .to_string(),
    )
    .expect("replace exact completion artifact");
}
