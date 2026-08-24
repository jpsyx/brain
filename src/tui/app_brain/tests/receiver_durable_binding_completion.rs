use super::receiver_durable_support::{accept_email_job, publish_valid_completion};
use super::*;

use crate::state::ReceiverJobState;

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
        ReceiverJobState::Launching,
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
        ReceiverJobState::Launching,
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
            "message": "completed after durable binding repair",
        })
        .to_string(),
    )
    .expect("replace exact completion artifact");
}
