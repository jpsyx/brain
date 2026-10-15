//! Actual pre-receiver v5 tables, before the receiver migration adds its schema.
use std::path::Path;

pub(super) fn seed(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE brain_sessions (
            agent_kind TEXT NOT NULL,
            agent_session_id TEXT NOT NULL,
            brain_instance_id TEXT NOT NULL,
            locked_pid INTEGER,
            source TEXT,
            workspace_id TEXT NOT NULL,
            actor_id TEXT NOT NULL,
            channel TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            last_active_at INTEGER NOT NULL,
            completion_status TEXT NOT NULL DEFAULT 'active',
            PRIMARY KEY (agent_kind, agent_session_id, workspace_id, actor_id, channel)
        );
        CREATE INDEX brain_sessions_by_active ON brain_sessions
            (agent_kind, workspace_id, actor_id, channel, locked_pid, last_active_at);
        CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        PRAGMA user_version = 5;",
        )
        .unwrap();
}
