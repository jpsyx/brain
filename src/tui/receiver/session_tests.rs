use crate::agent::{AgentSession, SessionScope, SessionStore};
use crate::state::Db;

use super::{ReceiverRemoteSession, ReceiverSessionRegistration};

fn scope(frontend: crate::agent::AgentKind) -> SessionScope {
    let users = crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: crate::users::UserId::parse("test-user").expect("user ID"),
            name: "Test user".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    let actor = crate::actor::resolve_actor(
        &crate::users::UserId::parse("test-user").expect("user ID"),
        crate::actor::RequestIdentity::Local,
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

#[test]
fn remote_session_owners_are_unique_and_never_reuse_the_interactive_instance() {
    let first = ReceiverRemoteSession::new("interactive-shell");
    let second = ReceiverRemoteSession::new("interactive-shell");

    assert_ne!(first.instance(), "interactive-shell");
    assert_ne!(second.instance(), "interactive-shell");
    assert_ne!(first.instance(), second.instance());
    assert_ne!(first.placeholder(), second.placeholder());
}

#[test]
fn fresh_registration_guard_releases_only_the_exact_remote_owner_unless_committed() {
    let db = Db::open_in_memory().expect("state DB");
    let scope = scope(crate::agent::AgentKind::Codex);
    let main_session = AgentSession::new("main-session").expect("main session");
    SessionStore::register(&db, &main_session, "interactive-shell", 41, &scope)
        .expect("register main session");
    let remote = ReceiverRemoteSession::new("interactive-shell");
    {
        let guard = ReceiverSessionRegistration::register_fresh(&db, &remote, 42, &scope)
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
    ReceiverSessionRegistration::register_fresh(&db, &committed, 43, &scope)
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
    let candidate = AgentSession::new("native-session").expect("native session");
    SessionStore::register(&db, &candidate, "old-owner", 10, &scope).expect("register candidate");
    SessionStore::release(&db, "old-owner").expect("release candidate");
    let remote = ReceiverRemoteSession::new("interactive-shell");

    let guard = ReceiverSessionRegistration::claim_resume(&db, &remote, &candidate, 42, &scope)
        .expect("claim resume session")
        .expect("exact candidate is free");
    assert_eq!(
        db.locked_session_for_instance(remote.instance(), &scope)
            .as_deref(),
        Some("native-session")
    );
    assert!(
        ReceiverSessionRegistration::claim_resume(&db, &remote, &candidate, 42, &scope)
            .expect("reject second exact claim")
            .is_none()
    );
    drop(guard);
}
