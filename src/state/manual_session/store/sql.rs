use anyhow::{Context as _, Result};
use rusqlite::{OptionalExtension as _, Transaction};

use crate::agent::{AgentSession, SessionScope};
use crate::manual_session::{
    ManualSessionId, ManualSessionName, ManualSessionRecord, ManualSessionRole,
};

pub(super) fn immediate_transaction(connection: &rusqlite::Connection) -> Result<Transaction<'_>> {
    Ok(rusqlite::Transaction::new_unchecked(
        connection,
        rusqlite::TransactionBehavior::Immediate,
    )?)
}

pub(super) fn insert_brain_session(
    transaction: &Transaction<'_>,
    record: &ManualSessionRecord,
    pid: i32,
    scope: &SessionScope,
    workspace_id: &str,
    now: i64,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO brain_sessions
           (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
            workspace_id, actor_id, channel, created_at, last_active_at)
         VALUES (?1, ?2, ?3, ?4, 'fresh', ?5, ?6, ?7, ?8, ?8)",
        rusqlite::params![
            scope.agent_kind().as_str(),
            record.agent_session().as_str(),
            record.id().as_str(),
            pid,
            workspace_id,
            scope.actor().user_id().as_str(),
            scope.actor().channel().as_str(),
            now,
        ],
    )?;
    Ok(())
}

pub(super) fn insert_mapping(
    transaction: &Transaction<'_>,
    record: &ManualSessionRecord,
    scope: &SessionScope,
    workspace_id: &str,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO manual_sessions
           (manual_session_id, agent_kind, agent_session_id, workspace_id,
            actor_id, channel, title, position, role)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            record.id().as_str(),
            scope.agent_kind().as_str(),
            record.agent_session().as_str(),
            workspace_id,
            scope.actor().user_id().as_str(),
            scope.actor().channel().as_str(),
            record.name().as_str(),
            i64::from(record.position()),
            record.role().as_str(),
        ],
    )?;
    Ok(())
}

pub(super) fn load_mapping(
    transaction: &Transaction<'_>,
    id: &ManualSessionId,
    scope: &SessionScope,
    workspace_id: &str,
) -> Result<Option<ManualSessionRecord>> {
    let row = transaction
        .query_row(
            "SELECT manual_session_id, agent_session_id, title, position, role
             FROM manual_sessions
             WHERE manual_session_id = ?1 AND agent_kind = ?2
               AND workspace_id = ?3 AND actor_id = ?4 AND channel = ?5",
            rusqlite::params![
                id.as_str(),
                scope.agent_kind().as_str(),
                workspace_id,
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    row.map(decode_record).transpose()
}

pub(super) fn decode_record(
    (id, agent_session, title, position, role): (String, String, String, i64, String),
) -> Result<ManualSessionRecord> {
    Ok(ManualSessionRecord {
        id: ManualSessionId::parse(&id).context("manual session id is blank")?,
        agent_session: AgentSession::new(agent_session)?,
        name: ManualSessionName::parse(&title, &[])?,
        position: u32::try_from(position).context("manual session position is out of range")?,
        role: ManualSessionRole::parse(&role).context("manual session role is invalid")?,
    })
}

pub(super) fn release_native_session(
    transaction: &Transaction<'_>,
    id: &ManualSessionId,
    session: &AgentSession,
    scope: &SessionScope,
    workspace_id: &str,
    now: i64,
) -> Result<usize> {
    Ok(transaction.execute(
        "UPDATE brain_sessions SET locked_pid = NULL, last_active_at = ?7
         WHERE agent_kind = ?1 AND agent_session_id = ?2
           AND brain_instance_id = ?3 AND workspace_id = ?4
           AND actor_id = ?5 AND channel = ?6",
        rusqlite::params![
            scope.agent_kind().as_str(),
            session.as_str(),
            id.as_str(),
            workspace_id,
            scope.actor().user_id().as_str(),
            scope.actor().channel().as_str(),
            now,
        ],
    )?)
}

pub(super) fn compact_later_positions(
    transaction: &Transaction<'_>,
    closed_position: u32,
    scope: &SessionScope,
    workspace_id: &str,
) -> Result<()> {
    let later = {
        let mut statement = transaction.prepare(
            "SELECT manual_session_id, position FROM manual_sessions
             WHERE agent_kind = ?1 AND workspace_id = ?2
               AND actor_id = ?3 AND channel = ?4
               AND role = 'additional' AND position > ?5
             ORDER BY position",
        )?;
        let rows = statement.query_map(
            rusqlite::params![
                scope.agent_kind().as_str(),
                workspace_id,
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
                i64::from(closed_position),
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (manual_session_id, position) in later {
        transaction.execute(
            "UPDATE manual_sessions SET position = ?1
             WHERE manual_session_id = ?2 AND agent_kind = ?3
               AND workspace_id = ?4 AND actor_id = ?5 AND channel = ?6",
            rusqlite::params![
                position - 1,
                manual_session_id,
                scope.agent_kind().as_str(),
                workspace_id,
                scope.actor().user_id().as_str(),
                scope.actor().channel().as_str(),
            ],
        )?;
    }
    Ok(())
}
