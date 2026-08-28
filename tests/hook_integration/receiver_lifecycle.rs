use super::*;

use brain::actor::{RequestIdentity, resolve_actor};
use brain::agent::{AgentKind, AgentSession, SessionScope, SessionStore};
use brain::server::receiver::{Channel, InboundJob};
use brain::state::{ReceiverConversationIdentity, ReceiverSessionAttribution};
use brain::users::{USERS_SCHEMA_VERSION, User, UserId, Users};
use brain::workspace::WorkspaceId;

#[test]
fn late_exact_start_cannot_relock_a_released_receiver_session() {
    let fixture = registered_receiver(
        AgentKind::Claude,
        "released-native-session",
        "released-instance",
    );
    assert!(
        SessionStore::mark_completed(&fixture.db, &fixture.session, &fixture.scope)
            .expect("mark session completed")
    );
    fixture
        .db
        .release_receiver_session(&fixture.registration)
        .expect("release exact receiver registration");

    let output = run_receiver_hook(&fixture, fixture.session.as_str());

    assert!(output.status.success(), "hook failed: {output:?}");
    assert_eq!(
        session_lifecycle(&fixture, fixture.session.as_str()),
        (None, "completed".to_owned())
    );
    assert_eq!(registration_count(&fixture), 0);
}

#[test]
fn live_exact_receiver_start_remains_authorized_for_every_frontend() {
    for frontend in [AgentKind::Claude, AgentKind::Codex, AgentKind::OpenCode] {
        let fixture = registered_receiver(frontend, "bound-native-session", "live-instance");
        assert!(
            SessionStore::mark_completed(&fixture.db, &fixture.session, &fixture.scope)
                .expect("mark session completed")
        );

        let output = run_receiver_hook(&fixture, fixture.session.as_str());

        assert!(
            output.status.success(),
            "{} hook failed: {output:?}",
            frontend.as_str()
        );
        assert_eq!(
            session_lifecycle(&fixture, fixture.session.as_str()),
            (Some(4242), "active".to_owned())
        );
        assert_eq!(registration_count(&fixture), 1);
    }
}

#[test]
fn live_receiver_rotation_and_exact_refire_remain_authorized_for_every_frontend() {
    for frontend in [AgentKind::Claude, AgentKind::Codex, AgentKind::OpenCode] {
        let fixture = registered_receiver(frontend, "pending-session", "rotating-instance");

        let rotated = run_receiver_hook(&fixture, "native-session");
        let refired = run_receiver_hook(&fixture, "native-session");

        assert!(
            rotated.status.success(),
            "{} rotation failed: {rotated:?}",
            frontend.as_str()
        );
        assert!(
            refired.status.success(),
            "{} refire failed: {refired:?}",
            frontend.as_str()
        );
        assert_eq!(
            session_lifecycle(&fixture, fixture.session.as_str()),
            (None, "active".to_owned())
        );
        assert_eq!(
            session_lifecycle(&fixture, "native-session"),
            (Some(4242), "active".to_owned())
        );
        assert_eq!(registration_count(&fixture), 1);
    }
}

struct ReceiverFixture {
    _temporary: tempfile::TempDir,
    state_path: PathBuf,
    db: Db,
    workspace_id: WorkspaceId,
    frontend: AgentKind,
    scope: SessionScope,
    session: AgentSession,
    registration: ReceiverSessionAttribution,
}

fn registered_receiver(frontend: AgentKind, session_id: &str, instance: &str) -> ReceiverFixture {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let state_path = temporary.path().join("state.db");
    let workspace_id =
        WorkspaceId::parse("11111111-1111-4111-8111-111111111111").expect("workspace ID");
    let actor = sms_actor();
    let db = Db::open_path_with_legacy_identity(&state_path, &workspace_id.to_string(), "pablo")
        .expect("state DB");
    let inbound = InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id,
        actor: actor.clone(),
        channel: Channel::Sms,
        authenticated_sender: "+12125550100".to_owned(),
        response_sender: "+13105550100".to_owned(),
        prompt: "receiver lifecycle".to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms: 100,
        provider_id: Some(format!("receiver-{session_id}")),
        thread_participants: vec!["+12125550100".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    };
    let identity = ReceiverConversationIdentity::sms(workspace_id, actor.user_id().clone());
    let accepted = db
        .accept_receiver_job(&inbound, &identity)
        .expect("accept receiver job");
    let scope = SessionScope::new(frontend, workspace_id, actor);
    let session = AgentSession::new(session_id).expect("session ID");
    let registration = db
        .register_receiver_session(accepted.conversation_id(), &session, instance, 4242, &scope)
        .expect("register exact receiver session");
    ReceiverFixture {
        _temporary: temporary,
        state_path,
        db,
        workspace_id,
        frontend,
        scope,
        session,
        registration,
    }
}

fn run_receiver_hook(fixture: &ReceiverFixture, session_id: &str) -> std::process::Output {
    let mut command = scoped_hook_command(
        &fixture.state_path,
        fixture.frontend.as_str(),
        "pablo",
        fixture.registration.instance(),
    );
    command.env("BRAIN_CHANNEL", "sms");
    run_hook_command(command, &start_input(session_id))
}

fn session_lifecycle(fixture: &ReceiverFixture, session_id: &str) -> (Option<i64>, String) {
    Connection::open(&fixture.state_path)
        .expect("read state DB")
        .query_row(
            "SELECT locked_pid, completion_status FROM brain_sessions
             WHERE agent_kind = ?1 AND agent_session_id = ?2
               AND workspace_id = ?3 AND actor_id = 'pablo' AND channel = 'sms'",
            rusqlite::params![
                fixture.frontend.as_str(),
                session_id,
                fixture.workspace_id.to_string()
            ],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("session row remains durable")
}

fn registration_count(fixture: &ReceiverFixture) -> i64 {
    Connection::open(&fixture.state_path)
        .expect("read state DB")
        .query_row(
            "SELECT COUNT(*) FROM receiver_session_registrations
             WHERE workspace_id = ?1 AND brain_instance_id = ?2",
            rusqlite::params![
                fixture.workspace_id.to_string(),
                fixture.registration.instance()
            ],
            |row| row.get::<_, i64>(0),
        )
        .expect("count receiver registrations")
}

fn sms_actor() -> brain::actor::ActorContext {
    let user_id = UserId::parse("pablo").expect("user ID");
    let users = Users {
        schema_version: USERS_SCHEMA_VERSION,
        users: vec![User {
            id: user_id.clone(),
            name: "Pablo".to_owned(),
            phones: vec![brain::users::PhoneIdentity {
                value: "+12125550100".to_owned(),
                inbound_allowed: true,
            }],
            emails: Vec::new(),
            response_email: None,
        }],
    };
    resolve_actor(
        &user_id,
        RequestIdentity::Sms {
            from: "+12125550100",
        },
        &users,
    )
    .expect("receiver actor")
}
