use anyhow::Result;
use rusqlite::Connection;

pub(super) const VERSION: i32 = 14;

pub(in crate::state) fn up(connection: &Connection, current_version: i32) -> Result<()> {
    let transaction =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)?;
    let stored_version: i32 =
        transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current_version > VERSION || stored_version > VERSION {
        transaction.commit()?;
        return Ok(());
    }
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS manual_sessions (
           manual_session_id TEXT NOT NULL,
           agent_kind        TEXT NOT NULL CHECK (agent_kind IN ('claude', 'codex', 'opencode')),
           agent_session_id  TEXT NOT NULL,
           workspace_id      TEXT NOT NULL,
           actor_id          TEXT NOT NULL,
           channel           TEXT NOT NULL CHECK (channel = 'interactive'),
           title             TEXT NOT NULL COLLATE NOCASE,
           position          INTEGER NOT NULL CHECK (position >= 0),
           role              TEXT NOT NULL CHECK (role IN ('main', 'additional')),
           PRIMARY KEY (agent_kind, workspace_id, actor_id, channel, manual_session_id),
           UNIQUE (agent_kind, workspace_id, actor_id, channel, title),
           UNIQUE (agent_kind, workspace_id, actor_id, channel, position),
           CHECK ((role = 'main' AND position = 0) OR
                  (role = 'additional' AND position > 0)),
           FOREIGN KEY (agent_kind, agent_session_id, workspace_id, actor_id, channel)
             REFERENCES brain_sessions(agent_kind, agent_session_id, workspace_id, actor_id, channel)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS manual_sessions_one_main
           ON manual_sessions(agent_kind, workspace_id, actor_id, channel)
           WHERE role = 'main';",
    )?;
    if stored_version != VERSION {
        transaction.pragma_update(None, "user_version", VERSION)?;
    }
    transaction.commit()?;
    Ok(())
}

pub(crate) fn down_path(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let connection = Connection::open(path)?;
    crate::state::Db::configure(&connection)?;
    let transaction = rusqlite::Transaction::new_unchecked(
        &connection,
        rusqlite::TransactionBehavior::Immediate,
    )?;
    let version: i32 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == VERSION {
        transaction.execute_batch(
            "DROP INDEX IF EXISTS manual_sessions_one_main;
             DROP TABLE IF EXISTS manual_sessions;
             PRAGMA user_version = 13;",
        )?;
    }
    transaction.commit()?;
    Ok(())
}
