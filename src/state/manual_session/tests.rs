use super::*;
use crate::agent::{AgentKind, AgentSession, SessionScope};
use crate::manual_session::{
    ManualSessionId, ManualSessionName, ManualSessionRecord, ManualSessionRole,
};
use crate::state::Db;

fn interactive_scope() -> SessionScope {
    let users = crate::users::Users {
        schema_version: crate::users::USERS_SCHEMA_VERSION,
        users: vec![crate::users::User {
            id: crate::users::UserId::parse("test-user").unwrap(),
            name: "Test user".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    let actor = crate::actor::resolve_actor(
        &crate::users::UserId::parse("test-user").unwrap(),
        crate::actor::RequestIdentity::Local,
        &users,
    )
    .unwrap();
    SessionScope::new(
        AgentKind::Claude,
        crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        actor,
    )
}

fn record(
    id: &str,
    native_id: &str,
    title: &str,
    position: u32,
    role: ManualSessionRole,
) -> ManualSessionRecord {
    ManualSessionRecord {
        id: ManualSessionId::parse(id).unwrap(),
        agent_session: AgentSession::new(native_id).unwrap(),
        name: ManualSessionName::parse(title, &[]).unwrap(),
        position,
        role,
    }
}

#[test]
fn rollback_fresh_additional_removes_both_exact_rows_and_preserves_neighbors() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let main =
        ManualSessionRecord::main(ManualSessionId::new(), AgentSession::new("main").unwrap());
    let atlas = record(
        "atlas",
        "atlas-native",
        "Atlas",
        1,
        ManualSessionRole::Additional,
    );
    let beacon = record(
        "beacon",
        "beacon-native",
        "Beacon",
        2,
        ManualSessionRole::Additional,
    );
    for record in [&main, &atlas, &beacon] {
        db.register_fresh_manual_session(record, 42, &scope)
            .unwrap();
    }

    db.rollback_fresh_manual_session(&atlas, 42, &scope)
        .unwrap();

    let remaining = db.manual_sessions(&scope).unwrap();
    assert_eq!(
        remaining
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        ["Brain", "Beacon"]
    );
    assert_eq!(
        db.conn
            .query_row("SELECT count(*) FROM brain_sessions", [], |row| row
                .get::<_, u32>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.locked_session_for_instance("beacon", &scope),
        Some("beacon-native".to_owned())
    );
}

#[test]
fn rollback_rejects_main_wrong_native_and_wrong_owner_without_removing_either_row() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let main =
        ManualSessionRecord::main(ManualSessionId::new(), AgentSession::new("main").unwrap());
    let atlas = record(
        "atlas",
        "atlas-native",
        "Atlas",
        1,
        ManualSessionRole::Additional,
    );
    for record in [&main, &atlas] {
        db.register_fresh_manual_session(record, 42, &scope)
            .unwrap();
    }
    let before = db.manual_sessions(&scope).unwrap();
    let mut mismatched = atlas.clone();
    mismatched.agent_session = AgentSession::new("other-native").unwrap();

    assert!(db.rollback_fresh_manual_session(&main, 42, &scope).is_err());
    assert!(
        db.rollback_fresh_manual_session(&mismatched, 42, &scope)
            .is_err()
    );
    assert!(
        db.rollback_fresh_manual_session(&atlas, 43, &scope)
            .is_err()
    );

    assert_eq!(db.manual_sessions(&scope).unwrap(), before);
    assert_eq!(
        db.conn
            .query_row("SELECT count(*) FROM brain_sessions", [], |row| row
                .get::<_, u32>(0))
            .unwrap(),
        2
    );
}

#[test]
fn manual_sessions_round_trip_in_main_then_position_order() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let main = record(
        "main-id",
        "main-native",
        "Brain",
        0,
        ManualSessionRole::Main,
    );
    let atlas = record(
        "atlas-id",
        "atlas-native",
        "Atlas",
        1,
        ManualSessionRole::Additional,
    );

    db.register_fresh_manual_session(&atlas, 42, &scope)
        .unwrap();
    db.register_fresh_manual_session(&main, 42, &scope).unwrap();

    assert_eq!(db.manual_sessions(&scope).unwrap(), vec![main, atlas]);
}

#[test]
fn closing_one_manual_session_releases_only_its_lock_and_compacts_positions() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let main = record(
        "main-id",
        "main-native",
        "Brain",
        0,
        ManualSessionRole::Main,
    );
    let first = record(
        "first-id",
        "first-native",
        "First",
        1,
        ManualSessionRole::Additional,
    );
    let second = record(
        "second-id",
        "second-native",
        "Second",
        2,
        ManualSessionRole::Additional,
    );
    for session in [&main, &first, &second] {
        db.register_fresh_manual_session(session, 42, &scope)
            .unwrap();
    }

    db.close_manual_session(first.id(), &scope).unwrap();

    assert_eq!(
        db.manual_sessions(&scope).unwrap(),
        vec![
            main,
            record(
                "second-id",
                "second-native",
                "Second",
                1,
                ManualSessionRole::Additional,
            ),
        ]
    );
    assert_eq!(db.locked_session_for_instance("first-id", &scope), None);
    assert_eq!(
        db.locked_session_for_instance("second-id", &scope),
        Some("second-native".to_owned())
    );
}

#[test]
fn schema_rejects_second_main_zero_position_additional_and_duplicate_title() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let main = record(
        "main-id",
        "main-native",
        "Brain",
        0,
        ManualSessionRole::Main,
    );
    db.register_fresh_manual_session(&main, 42, &scope).unwrap();

    let second_main = record(
        "other-main",
        "other-native",
        "Other",
        0,
        ManualSessionRole::Main,
    );
    assert!(
        db.register_fresh_manual_session(&second_main, 42, &scope)
            .is_err()
    );
    assert_eq!(db.locked_session_for_instance("other-main", &scope), None);

    let zero = record(
        "zero-id",
        "zero-native",
        "Zero",
        0,
        ManualSessionRole::Additional,
    );
    assert!(db.register_fresh_manual_session(&zero, 42, &scope).is_err());
    assert_eq!(db.locked_session_for_instance("zero-id", &scope), None);

    let atlas = record(
        "atlas-id",
        "atlas-native",
        "Atlas",
        1,
        ManualSessionRole::Additional,
    );
    db.register_fresh_manual_session(&atlas, 42, &scope)
        .unwrap();
    let duplicate = record(
        "duplicate-id",
        "duplicate-native",
        "aTlAs",
        2,
        ManualSessionRole::Additional,
    );
    assert!(
        db.register_fresh_manual_session(&duplicate, 42, &scope)
            .is_err()
    );
    assert_eq!(db.locked_session_for_instance("duplicate-id", &scope), None);
}

#[test]
fn cross_scope_mutation_is_rejected_without_changing_the_mapping_or_lock() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let atlas = record(
        "atlas-id",
        "atlas-native",
        "Atlas",
        1,
        ManualSessionRole::Additional,
    );
    db.register_fresh_manual_session(&atlas, 42, &scope)
        .unwrap();
    let other_scope = SessionScope::new(
        AgentKind::OpenCode,
        scope.workspace_id(),
        scope.actor().clone(),
    );

    assert!(db.close_manual_session(atlas.id(), &other_scope).is_err());

    assert_eq!(db.manual_sessions(&scope).unwrap(), vec![atlas]);
    assert_eq!(
        db.locked_session_for_instance("atlas-id", &scope),
        Some("atlas-native".to_owned())
    );
}

#[test]
fn replace_manual_session_updates_only_the_exact_native_binding() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let atlas = record(
        "atlas-id",
        "old-native",
        "Atlas",
        1,
        ManualSessionRole::Additional,
    );
    db.register_fresh_manual_session(&atlas, 42, &scope)
        .unwrap();
    let replacement = AgentSession::new("new-native").unwrap();

    let updated = db
        .replace_manual_session(atlas.id(), &replacement, 84, &scope)
        .unwrap();

    assert_eq!(updated.agent_session(), &replacement);
    assert_eq!(db.manual_sessions(&scope).unwrap(), vec![updated]);
    assert_eq!(
        db.locked_session_for_instance("atlas-id", &scope),
        Some("new-native".to_owned())
    );
    assert!(
        db.sessions_by_recency(&scope)
            .contains(&"old-native".to_owned())
    );
}

#[test]
fn release_manual_session_preserves_its_mapping() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let atlas = record(
        "atlas-id",
        "atlas-native",
        "Atlas",
        1,
        ManualSessionRole::Additional,
    );
    db.register_fresh_manual_session(&atlas, 42, &scope)
        .unwrap();

    db.release_manual_session(atlas.id(), &scope).unwrap();

    assert_eq!(db.manual_sessions(&scope).unwrap(), vec![atlas]);
    assert_eq!(db.locked_session_for_instance("atlas-id", &scope), None);
}

#[test]
fn attaching_requires_the_exact_native_row_to_be_claimed_by_the_manual_identity() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let atlas = record(
        "atlas-id",
        "atlas-native",
        "Atlas",
        1,
        ManualSessionRole::Additional,
    );
    db.register_scoped_fresh("atlas-native", "other-id", 42, &scope)
        .unwrap();
    assert!(db.attach_manual_session(&atlas, &scope).is_err());
    assert!(db.manual_sessions(&scope).unwrap().is_empty());

    db.release("other-id").unwrap();
    assert!(db.claim("atlas-native", "atlas-id", 84, &scope).unwrap());
    db.attach_manual_session(&atlas, &scope).unwrap();
    assert_eq!(db.manual_sessions(&scope).unwrap(), vec![atlas]);
}

#[test]
fn closing_main_returns_a_typed_invariant_error() {
    let db = Db::open_in_memory().unwrap();
    let scope = interactive_scope();
    let main = record(
        "main-id",
        "main-native",
        "Brain",
        0,
        ManualSessionRole::Main,
    );
    db.register_fresh_manual_session(&main, 42, &scope).unwrap();

    let error = db.close_manual_session(main.id(), &scope).unwrap_err();

    assert_eq!(
        error.downcast_ref::<store::ManualSessionStoreError>(),
        Some(&store::ManualSessionStoreError::MainCannotClose)
    );
    assert_eq!(db.manual_sessions(&scope).unwrap(), vec![main]);
}

#[test]
fn down_migration_drops_manual_sessions_and_preserves_brain_sessions() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("state.db");
    let db = Db::open_path_with_legacy_identity(
        &path,
        "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        "test-user",
    )
    .unwrap();
    let scope = interactive_scope();
    let main = record(
        "main-id",
        "main-native",
        "Brain",
        0,
        ManualSessionRole::Main,
    );
    db.register_fresh_manual_session(&main, 42, &scope).unwrap();
    drop(db);

    schema::down_path(&path).unwrap();

    let connection = rusqlite::Connection::open(&path).unwrap();
    let manual_table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'manual_sessions'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let brain_session_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM brain_sessions", [], |row| row.get(0))
        .unwrap();
    let version: i32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(manual_table_count, 0);
    assert_eq!(brain_session_count, 1);
    assert_eq!(version, 13);
}
