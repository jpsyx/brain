use std::path::Path;

use brain::actor::{Channel, RequestIdentity, resolve_actor};
use brain::session::AgentKind;
use brain::state::{Db, SessionScope};
use brain::users::{USERS_SCHEMA_VERSION, User, UserId, Users};
use brain::workspace::WorkspaceId;
use rusqlite::Connection;

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

fn actor(id: &str) -> brain::actor::ActorContext {
    let users = Users {
        schema_version: USERS_SCHEMA_VERSION,
        users: vec![User {
            id: UserId::parse(id).unwrap(),
            name: "Workspace member".to_owned(),
            phones: Vec::new(),
            emails: Vec::new(),
            response_email: None,
        }],
    };
    resolve_actor(&UserId::parse(id).unwrap(), RequestIdentity::Local, &users).unwrap()
}

fn create_legacy_v2(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE brain_sessions (
           claude_session_id TEXT PRIMARY KEY,
           brain_instance_id TEXT NOT NULL,
           locked_pid INTEGER,
           source TEXT,
           created_at INTEGER NOT NULL,
           last_active_at INTEGER NOT NULL,
           channel TEXT NOT NULL DEFAULT 'interactive'
         );
         CREATE INDEX brain_sessions_by_active
           ON brain_sessions(locked_pid, last_active_at);
         CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         INSERT INTO brain_sessions VALUES
           ('legacy-session', 'legacy-instance', NULL, 'resume', 10, 20, 'interactive');
         PRAGMA user_version = 2;",
    )
    .unwrap();
}

#[test]
fn migration_preserves_legacy_rows_as_local_interactive_claude_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");
    create_legacy_v2(&path);

    drop(
        Db::open_path_with_legacy_identity(&path, WORKSPACE_ID, "pablo")
            .expect("migrate legacy state"),
    );

    let conn = Connection::open(path).unwrap();
    let row = conn
        .query_row(
            "SELECT agent_kind, agent_session_id, workspace_id, actor_id, channel
             FROM brain_sessions",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        row,
        (
            "claude".to_owned(),
            "legacy-session".to_owned(),
            WORKSPACE_ID.to_owned(),
            "pablo".to_owned(),
            "interactive".to_owned(),
        )
    );
}

#[test]
fn scoped_follow_ups_never_resume_another_actor_or_frontend() {
    let db = Db::open_path(&tempfile::tempdir().unwrap().path().join("state.db")).unwrap();
    let workspace = WorkspaceId::parse(WORKSPACE_ID).unwrap();
    let pablo = SessionScope::new(AgentKind::Claude, workspace, actor("pablo"));
    let partner = SessionScope::new(AgentKind::Claude, workspace, actor("partner"));
    let codex = SessionScope::new(AgentKind::Codex, workspace, actor("pablo"));
    db.register_scoped_fresh("pablo-claude", "i1", 11, &pablo)
        .unwrap();
    db.register_scoped_fresh("partner-claude", "i2", 12, &partner)
        .unwrap();
    db.register_scoped_fresh("pablo-codex", "i3", 13, &codex)
        .unwrap();
    db.release("i1").unwrap();
    db.release("i2").unwrap();
    db.release("i3").unwrap();

    assert_eq!(db.sessions_by_recency(&pablo), vec!["pablo-claude"]);
    assert_eq!(db.sessions_by_recency(&partner), vec!["partner-claude"]);
    assert_eq!(db.sessions_by_recency(&codex), vec!["pablo-codex"]);
    assert_eq!(pablo.actor().channel(), Channel::Interactive);
}

#[test]
fn equal_opaque_session_ids_are_preserved_in_distinct_immutable_scopes() {
    let db = Db::open_path(&tempfile::tempdir().unwrap().path().join("state.db")).unwrap();
    let workspace = WorkspaceId::parse(WORKSPACE_ID).unwrap();
    let claude = SessionScope::new(AgentKind::Claude, workspace, actor("pablo"));
    let codex = SessionScope::new(AgentKind::Codex, workspace, actor("pablo"));
    let partner = SessionScope::new(AgentKind::Claude, workspace, actor("partner"));

    db.register_scoped_fresh("same-opaque-id", "claude-instance", 11, &claude)
        .unwrap();
    db.register_scoped_fresh("same-opaque-id", "codex-instance", 12, &codex)
        .unwrap();
    db.register_scoped_fresh("same-opaque-id", "partner-instance", 13, &partner)
        .unwrap();
    db.release("claude-instance").unwrap();
    db.release("codex-instance").unwrap();
    db.release("partner-instance").unwrap();

    assert_eq!(db.sessions_by_recency(&claude), ["same-opaque-id"]);
    assert_eq!(db.sessions_by_recency(&codex), ["same-opaque-id"]);
    assert_eq!(db.sessions_by_recency(&partner), ["same-opaque-id"]);
}

#[test]
fn claiming_an_equal_opaque_id_locks_only_the_requested_scope() {
    let db = Db::open_path(&tempfile::tempdir().unwrap().path().join("state.db")).unwrap();
    let workspace = WorkspaceId::parse(WORKSPACE_ID).unwrap();
    let claude = SessionScope::new(AgentKind::Claude, workspace, actor("pablo"));
    let codex = SessionScope::new(AgentKind::Codex, workspace, actor("pablo"));
    db.register_scoped_fresh("same-opaque-id", "claude-instance", 11, &claude)
        .unwrap();
    db.register_scoped_fresh("same-opaque-id", "codex-instance", 12, &codex)
        .unwrap();
    db.release("claude-instance").unwrap();
    db.release("codex-instance").unwrap();

    assert!(
        db.claim("same-opaque-id", "claiming-codex", 21, &codex)
            .unwrap()
    );

    assert_eq!(db.sessions_by_recency(&claude), ["same-opaque-id"]);
    assert!(db.sessions_by_recency(&codex).is_empty());
}

#[test]
fn reaping_an_equal_opaque_id_unlocks_only_the_dead_scope() {
    let db = Db::open_path(&tempfile::tempdir().unwrap().path().join("state.db")).unwrap();
    let workspace = WorkspaceId::parse(WORKSPACE_ID).unwrap();
    let claude = SessionScope::new(AgentKind::Claude, workspace, actor("pablo"));
    let codex = SessionScope::new(AgentKind::Codex, workspace, actor("pablo"));
    let live_pid = i32::try_from(std::process::id()).unwrap();
    db.register_scoped_fresh("same-opaque-id", "dead-claude", i32::MAX, &claude)
        .unwrap();
    db.register_scoped_fresh("same-opaque-id", "live-codex", live_pid, &codex)
        .unwrap();

    db.reap_dead_locks().unwrap();

    assert_eq!(db.sessions_by_recency(&claude), ["same-opaque-id"]);
    assert!(db.sessions_by_recency(&codex).is_empty());
}
