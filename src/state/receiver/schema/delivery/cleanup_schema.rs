use anyhow::{Result, bail};
use rusqlite::Connection;

/// Substituted with the registered frontend names; not a format argument, so
/// the template stays a plain constant the contract check can compare against.
const AGENT_KINDS_PLACEHOLDER: &str = "%AGENT_KINDS%";

pub(in crate::state) const TABLE: &str = "receiver_answer_cleanups";
pub(in crate::state) const COLUMNS: &str =
    "job_id, job_token, workspace_id, conversation_id, brain_instance_id, agent_kind, actor_id, \
     channel, registered_session_id, actual_session_id, controller_shutdown_acknowledged, \
     session_released, artifacts_removed, created_at_unix_ms, updated_at_unix_ms";

fn create_table() -> String {
    create_table_with(&crate::state::frontend_contract::agent_kind_values())
}

pub(in crate::state) fn create_table_with(agent_kinds: &str) -> String {
    CREATE_TABLE_TEMPLATE.replace(AGENT_KINDS_PLACEHOLDER, agent_kinds)
}

const CREATE_TABLE_TEMPLATE: &str = "CREATE TABLE IF NOT EXISTS receiver_answer_cleanups (
           job_id                  TEXT PRIMARY KEY REFERENCES receiver_jobs(job_id) ON DELETE CASCADE,
           job_token               TEXT NOT NULL,
           workspace_id            TEXT NOT NULL,
           conversation_id         TEXT NOT NULL,
           brain_instance_id       TEXT NOT NULL,
           agent_kind              TEXT NOT NULL CHECK (agent_kind IN (%AGENT_KINDS%)),
           actor_id                TEXT NOT NULL,
           channel                 TEXT NOT NULL CHECK (channel IN ('sms', 'email')),
           registered_session_id   TEXT NOT NULL,
           actual_session_id       TEXT NOT NULL,
           controller_shutdown_acknowledged INTEGER NOT NULL DEFAULT 0
             CHECK (controller_shutdown_acknowledged IN (0, 1)),
           session_released        INTEGER NOT NULL DEFAULT 0 CHECK (session_released IN (0, 1)),
           artifacts_removed       INTEGER NOT NULL DEFAULT 0 CHECK (artifacts_removed IN (0, 1)),
           created_at_unix_ms      INTEGER NOT NULL,
           updated_at_unix_ms      INTEGER NOT NULL
         );";

pub(super) fn create(connection: &Connection) -> Result<()> {
    connection.execute_batch(&create_table())?;
    Ok(())
}

pub(super) fn ensure_optional_columns(connection: &Connection) -> Result<()> {
    let acknowledged_exists: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pragma_table_info('receiver_answer_cleanups')
           WHERE name = 'controller_shutdown_acknowledged'
         )",
        [],
        |row| row.get(0),
    )?;
    if !acknowledged_exists {
        connection.execute_batch(
            "ALTER TABLE receiver_answer_cleanups
             ADD COLUMN controller_shutdown_acknowledged INTEGER NOT NULL DEFAULT 0
               CHECK (controller_shutdown_acknowledged IN (0, 1));",
        )?;
    }
    connection.execute(
        "UPDATE receiver_answer_cleanups
         SET controller_shutdown_acknowledged = 1
         WHERE session_released = 1 AND controller_shutdown_acknowledged = 0",
        [],
    )?;
    Ok(())
}

pub(super) fn ensure_columns(connection: &Connection) -> Result<()> {
    for required in [
        "job_id",
        "job_token",
        "workspace_id",
        "conversation_id",
        "brain_instance_id",
        "agent_kind",
        "actor_id",
        "channel",
        "registered_session_id",
        "actual_session_id",
        "controller_shutdown_acknowledged",
        "session_released",
        "artifacts_removed",
        "created_at_unix_ms",
        "updated_at_unix_ms",
    ] {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM pragma_table_info('receiver_answer_cleanups') WHERE name = ?1
             )",
            [required],
            |row| row.get(0),
        )?;
        if !exists {
            bail!("receiver answer cleanup schema is missing required column {required}");
        }
    }
    Ok(())
}

pub(super) fn ensure_table_contract(connection: &Connection) -> Result<()> {
    let table_matches = crate::state::frontend_contract::table_contract_matches(
        connection,
        "receiver_answer_cleanups",
        &create_table(),
    );
    if table_matches && !has_legacy_instance_unique(connection)? {
        return Ok(());
    }
    connection.execute_batch(
        "ALTER TABLE receiver_answer_cleanups
           RENAME TO receiver_answer_cleanups_v12_rebuild;",
    )?;
    connection.execute_batch(&create_table())?;
    connection.execute_batch(
        "INSERT INTO receiver_answer_cleanups
           (job_id, job_token, workspace_id, conversation_id, brain_instance_id,
            agent_kind, actor_id, channel, registered_session_id, actual_session_id,
            controller_shutdown_acknowledged, session_released, artifacts_removed,
            created_at_unix_ms, updated_at_unix_ms)
         SELECT job_id, job_token, workspace_id, conversation_id, brain_instance_id,
                agent_kind, actor_id, channel, registered_session_id, actual_session_id,
                CASE WHEN session_released = 1 THEN 1
                     ELSE controller_shutdown_acknowledged END,
                session_released, artifacts_removed,
                created_at_unix_ms, updated_at_unix_ms
         FROM receiver_answer_cleanups_v12_rebuild;
         DROP TABLE receiver_answer_cleanups_v12_rebuild;",
    )?;
    Ok(())
}

fn has_legacy_instance_unique(connection: &Connection) -> Result<bool> {
    let mut statement = connection.prepare(
        "SELECT name FROM pragma_index_list('receiver_answer_cleanups')
         WHERE \"unique\" = 1 AND origin = 'c'",
    )?;
    let indexes = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    indexes.into_iter().try_fold(false, |found, index| {
        if found {
            return Ok(true);
        }
        let mut columns =
            connection.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
        let columns = columns
            .query_map([index], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(columns == ["workspace_id", "brain_instance_id"])
    })
}
