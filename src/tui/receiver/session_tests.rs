use crate::agent::{AgentSession, SessionScope, SessionStore};
use crate::state::Db;

use super::test_support::FailingReleaseStore;
use super::{ReceiverRemoteSession, ReceiverSessionRegistration};

fn scope(frontend: crate::agent::AgentKind) -> SessionScope {
    let users = crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: crate::users::UserId::parse("test-user").expect("user ID"),
            name: "Test user".to_owned(),
            phones: vec![crate::users::PhoneIdentity {
                value: "+12125550100".to_owned(),
                inbound_allowed: true,
            }],
            emails: Vec::new(),
            response_email: None,
        }],
    };
    let actor = crate::actor::resolve_actor(
        &crate::users::UserId::parse("test-user").expect("user ID"),
        crate::actor::RequestIdentity::Sms {
            from: "+12125550100",
        },
        &users,
    )
    .expect("actor");
    SessionScope::new(
        frontend,
        crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
            .expect("workspace ID"),
        actor,
    )
}

fn conversation_id(db: &Db, scope: &SessionScope) -> crate::state::ReceiverConversationId {
    let inbound = crate::server::receiver::InboundJob {
        job_id: uuid::Uuid::new_v4(),
        workspace_id: scope.workspace_id(),
        actor: scope.actor().clone(),
        channel: crate::server::receiver::Channel::Sms,
        authenticated_sender: "+12125550100".to_owned(),
        prompt: "private test prompt".to_owned(),
        attachments: Vec::new(),
        received_at_unix_ms: 100,
        provider_id: None,
        thread_participants: vec!["+12125550100".to_owned()],
        response_email: None,
        allowed_response_recipients: Vec::new(),
        email_reply: None,
    };
    let identity = crate::state::ReceiverConversationIdentity::sms(
        scope.workspace_id(),
        scope.actor().user_id().clone(),
    );
    db.accept_receiver_job(&inbound, &identity)
        .expect("accept receiver conversation")
        .conversation_id()
}

#[test]
fn remote_session_owners_are_unique_and_never_reuse_the_interactive_instance() {
    let first = ReceiverRemoteSession::new("interactive-shell");
    let second = ReceiverRemoteSession::new("interactive-shell");

    assert_ne!(first.instance(), "interactive-shell");
    assert_ne!(second.instance(), "interactive-shell");
    assert_ne!(first.instance(), second.instance());
    assert_ne!(first.placeholder(), second.placeholder());
    assert_eq!(
        uuid::Uuid::parse_str(first.instance())
            .expect("canonical receiver instance")
            .hyphenated()
            .to_string(),
        first.instance()
    );
}

#[test]
fn fresh_registration_guard_releases_only_the_exact_remote_owner_unless_committed() {
    let db = Db::open_in_memory().expect("state DB");
    let scope = scope(crate::agent::AgentKind::Codex);
    let main_session = AgentSession::new("main-session").expect("main session");
    SessionStore::register(&db, &main_session, "interactive-shell", 41, &scope)
        .expect("register main session");
    let conversation_id = conversation_id(&db, &scope);
    let remote = ReceiverRemoteSession::new("interactive-shell");
    {
        let guard =
            ReceiverSessionRegistration::register_fresh(&db, conversation_id, &remote, 42, &scope)
                .expect("register remote placeholder");
        assert_eq!(
            db.locked_session_for_instance(remote.instance(), &scope)
                .as_deref(),
            Some(remote.placeholder().as_str())
        );
        drop(guard);
    }
    assert!(
        db.locked_session_for_instance(remote.instance(), &scope)
            .is_none()
    );
    assert_eq!(
        db.locked_session_for_instance("interactive-shell", &scope)
            .as_deref(),
        Some("main-session")
    );

    let committed = ReceiverRemoteSession::new("interactive-shell");
    ReceiverSessionRegistration::register_fresh(&db, conversation_id, &committed, 43, &scope)
        .expect("register committed placeholder")
        .commit();
    assert_eq!(
        db.locked_session_for_instance(committed.instance(), &scope)
            .as_deref(),
        Some(committed.placeholder().as_str())
    );
}

#[test]
fn resume_registration_claims_only_the_exact_matching_native_session() {
    let db = Db::open_in_memory().expect("state DB");
    let scope = scope(crate::agent::AgentKind::OpenCode);
    let conversation_id = conversation_id(&db, &scope);
    let candidate = AgentSession::new("native-session").expect("native session");
    let binding = crate::state::ReceiverSessionBinding::new(
        crate::agent::AgentKind::OpenCode,
        candidate.as_str(),
    )
    .expect("native binding");
    db.update_receiver_conversation(conversation_id, "", Some(&binding), 100)
        .expect("seed exact receiver binding");
    SessionStore::register(&db, &candidate, "old-owner", 10, &scope).expect("register candidate");
    SessionStore::release(&db, "old-owner").expect("release candidate");
    let remote = ReceiverRemoteSession::new("interactive-shell");

    let guard = ReceiverSessionRegistration::claim_resume(
        &db,
        conversation_id,
        &remote,
        &candidate,
        42,
        &scope,
    )
    .expect("claim resume session")
    .expect("exact candidate is free");
    assert_eq!(
        db.locked_session_for_instance(remote.instance(), &scope)
            .as_deref(),
        Some("native-session")
    );
    assert!(
        ReceiverSessionRegistration::claim_resume(
            &db,
            conversation_id,
            &remote,
            &candidate,
            42,
            &scope,
        )
        .expect("reject second exact claim")
        .is_none()
    );
    drop(guard);
}

#[test]
fn explicit_registration_cleanup_surfaces_release_failure_before_drop_fallback() {
    let store = FailingReleaseStore::new();
    let scope = scope(crate::agent::AgentKind::Codex);
    let conversation_id = conversation_id(store.db(), &scope);
    let remote = ReceiverRemoteSession::new("interactive-shell");
    let guard =
        ReceiverSessionRegistration::register_fresh(&store, conversation_id, &remote, 42, &scope)
            .expect("register remote placeholder");

    let error = guard.cleanup().expect_err("surface release failure");

    assert_eq!(error.to_string(), "exact receiver release failed");
    assert_eq!(store.release_attempts(), 2);
}

#[test]
fn committed_registration_returns_the_exact_durable_attribution() {
    let db = Db::open_in_memory().expect("state DB");
    let scope = scope(crate::agent::AgentKind::Codex);
    let conversation_id = conversation_id(&db, &scope);
    let remote = ReceiverRemoteSession::new("interactive-shell");
    let registration =
        ReceiverSessionRegistration::register_fresh(&db, conversation_id, &remote, 42, &scope)
            .expect("register remote placeholder");

    let attribution = registration.commit();

    assert_eq!(attribution.conversation_id(), conversation_id);
    assert_eq!(attribution.instance(), remote.instance());
    assert_eq!(attribution.registered_session(), remote.placeholder());
    assert_eq!(attribution.scope(), &scope);
}
