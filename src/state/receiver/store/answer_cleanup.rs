use anyhow::Result;
use rusqlite::OptionalExtension as _;

use crate::state::{Db, ReceiverAnswerCleanup, ReceiverJobId};

use super::to_i64;

impl Db {
    /// Load one exact post-answer cleanup without changing its progress.
    pub fn receiver_answer_cleanup(
        &self,
        job_id: ReceiverJobId,
    ) -> Result<Option<ReceiverAnswerCleanup>> {
        load_cleanup(
            &self.conn,
            &self.workspace_id,
            "AND cleanup.job_id = ?2",
            Some(job_id),
        )
    }

    /// Load the oldest post-answer cleanup independently of agent FIFO work.
    pub fn next_receiver_answer_cleanup(&self) -> Result<Option<ReceiverAnswerCleanup>> {
        let sql = cleanup_select("");
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map([&self.workspace_id], load_cleanup_row)?;
        for row in rows {
            let cleanup = parse_cleanup_row(row?)?;
            if controller_handoff_is_eligible(&self.conn, &self.workspace_id, &cleanup)? {
                return Ok(Some(cleanup));
            }
        }
        Ok(None)
    }

    /// Persist the originating controller's exact successful shutdown handoff.
    pub fn acknowledge_receiver_answer_controller_shutdown(
        &self,
        job_id: crate::state::ReceiverJobId,
        token: crate::state::ReceiverJobToken,
        instance: &str,
        controller_pid: i32,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        anyhow::ensure!(
            controller_pid > 0,
            "receiver answer controller PID must be positive"
        );
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let changed = transaction.execute(
            "UPDATE receiver_answer_cleanups AS cleanup
             SET controller_shutdown_acknowledged = 1, updated_at_unix_ms = ?6
             WHERE cleanup.workspace_id = ?1 AND cleanup.job_id = ?2
               AND cleanup.job_token = ?3 AND cleanup.brain_instance_id = ?4
               AND cleanup.controller_shutdown_acknowledged = 0
               AND EXISTS (
                 SELECT 1
                 FROM receiver_session_registrations AS registration
                 JOIN brain_sessions AS session
                   ON session.workspace_id = registration.workspace_id
                  AND session.agent_kind = registration.agent_kind
                  AND session.actor_id = registration.actor_id
                  AND session.channel = registration.channel
                  AND session.brain_instance_id = registration.brain_instance_id
                  AND session.agent_session_id = registration.actual_session_id
                 WHERE registration.workspace_id = cleanup.workspace_id
                   AND registration.conversation_id = cleanup.conversation_id
                   AND registration.agent_kind = cleanup.agent_kind
                   AND registration.actor_id = cleanup.actor_id
                   AND registration.channel = cleanup.channel
                   AND registration.brain_instance_id = cleanup.brain_instance_id
                   AND registration.registered_session_id = cleanup.registered_session_id
                   AND registration.actual_session_id = cleanup.actual_session_id
                   AND session.locked_pid = ?5
               )",
            rusqlite::params![
                self.workspace_id,
                job_id.to_string(),
                token.to_string(),
                instance,
                controller_pid,
                to_i64(observed_at_unix_ms, "receiver controller shutdown time")?,
            ],
        )?;
        let acknowledged = if changed == 1 {
            true
        } else {
            transaction.query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM receiver_answer_cleanups
                   WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
                     AND brain_instance_id = ?4
                     AND controller_shutdown_acknowledged = 1
                 )",
                rusqlite::params![
                    self.workspace_id,
                    job_id.to_string(),
                    token.to_string(),
                    instance,
                ],
                |row| row.get(0),
            )?
        };
        transaction.commit()?;
        Ok(acknowledged)
    }

    /// Release only the exact session registration retained by a cleanup row.
    pub fn release_receiver_answer_cleanup_session(
        &self,
        cleanup: &ReceiverAnswerCleanup,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let stored = load_cleanup_identity(&transaction, &self.workspace_id, cleanup)?;
        let Some(stored) = stored else {
            return Ok(false);
        };
        if !controller_handoff_is_eligible(&transaction, &self.workspace_id, cleanup)? {
            return Ok(false);
        }
        if stored.session_released {
            transaction.commit()?;
            return Ok(true);
        }
        let proof: bool = transaction.query_row(
            "SELECT EXISTS(
               SELECT 1
               FROM receiver_session_registrations AS registration
               JOIN brain_sessions AS session
                 ON session.workspace_id = registration.workspace_id
                AND session.agent_kind = registration.agent_kind
                AND session.actor_id = registration.actor_id
                AND session.channel = registration.channel
                AND session.brain_instance_id = registration.brain_instance_id
                AND session.agent_session_id = registration.actual_session_id
               WHERE registration.workspace_id = ?1
                 AND registration.conversation_id = ?2
                 AND registration.agent_kind = ?3
                 AND registration.actor_id = ?4
                 AND registration.channel = ?5
                 AND registration.brain_instance_id = ?6
                 AND registration.registered_session_id = ?7
                 AND registration.actual_session_id = ?8
             )",
            rusqlite::params![
                self.workspace_id,
                stored.conversation_id,
                stored.agent_kind,
                stored.actor_id,
                stored.channel,
                cleanup.instance(),
                stored.registered_session_id,
                stored.actual_session_id,
            ],
            |row| row.get(0),
        )?;
        if !proof {
            return Ok(false);
        }
        transaction.execute(
            "UPDATE brain_sessions
             SET locked_pid = NULL, last_active_at = ?9
             WHERE workspace_id = ?1 AND agent_kind = ?2 AND actor_id = ?3
               AND channel = ?4 AND brain_instance_id = ?5
               AND agent_session_id = ?6
               AND EXISTS (
                 SELECT 1 FROM receiver_session_registrations
                 WHERE workspace_id = ?1 AND conversation_id = ?7
                   AND brain_instance_id = ?5 AND registered_session_id = ?8
               )",
            rusqlite::params![
                self.workspace_id,
                stored.agent_kind,
                stored.actor_id,
                stored.channel,
                cleanup.instance(),
                stored.actual_session_id,
                stored.conversation_id,
                stored.registered_session_id,
                to_i64(observed_at_unix_ms, "receiver answer cleanup time")?,
            ],
        )?;
        let registration_changed = transaction.execute(
            "DELETE FROM receiver_session_registrations
             WHERE workspace_id = ?1 AND conversation_id = ?2
               AND agent_kind = ?3 AND actor_id = ?4 AND channel = ?5
               AND brain_instance_id = ?6 AND registered_session_id = ?7
               AND actual_session_id = ?8",
            rusqlite::params![
                self.workspace_id,
                stored.conversation_id,
                stored.agent_kind,
                stored.actor_id,
                stored.channel,
                cleanup.instance(),
                stored.registered_session_id,
                stored.actual_session_id,
            ],
        )?;
        if registration_changed != 1 {
            return Ok(false);
        }
        let cleanup_changed = transaction.execute(
            "UPDATE receiver_answer_cleanups
             SET session_released = 1, updated_at_unix_ms = ?5
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND brain_instance_id = ?4 AND session_released = 0",
            rusqlite::params![
                self.workspace_id,
                cleanup.job_id().to_string(),
                cleanup.token().to_string(),
                cleanup.instance(),
                to_i64(observed_at_unix_ms, "receiver answer cleanup time")?,
            ],
        )?;
        if cleanup_changed != 1 {
            return Ok(false);
        }
        transaction.commit()?;
        Ok(true)
    }

    /// Record successful removal of only the exact response instance's files.
    pub fn mark_receiver_answer_artifacts_removed(
        &self,
        cleanup: &ReceiverAnswerCleanup,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if !controller_handoff_is_eligible(&transaction, &self.workspace_id, cleanup)? {
            return Ok(false);
        }
        let changed = transaction.execute(
            "UPDATE receiver_answer_cleanups
             SET artifacts_removed = 1, updated_at_unix_ms = ?5
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND brain_instance_id = ?4",
            rusqlite::params![
                self.workspace_id,
                cleanup.job_id().to_string(),
                cleanup.token().to_string(),
                cleanup.instance(),
                to_i64(observed_at_unix_ms, "receiver answer cleanup time")?,
            ],
        )? == 1;
        transaction.commit()?;
        Ok(changed)
    }

    /// Remove cleanup authority only after every local effect is complete.
    pub fn finish_receiver_answer_cleanup(&self, cleanup: &ReceiverAnswerCleanup) -> Result<bool> {
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.conn,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if !controller_handoff_is_eligible(&transaction, &self.workspace_id, cleanup)? {
            return Ok(false);
        }
        let changed = transaction.execute(
            "DELETE FROM receiver_answer_cleanups
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND brain_instance_id = ?4
               AND session_released = 1 AND artifacts_removed = 1",
            rusqlite::params![
                self.workspace_id,
                cleanup.job_id().to_string(),
                cleanup.token().to_string(),
                cleanup.instance(),
            ],
        )? == 1;
        transaction.commit()?;
        Ok(changed)
    }
}

struct StoredCleanupIdentity {
    conversation_id: String,
    agent_kind: String,
    actor_id: String,
    channel: String,
    registered_session_id: String,
    actual_session_id: String,
    session_released: bool,
}

fn load_cleanup_identity(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    cleanup: &ReceiverAnswerCleanup,
) -> Result<Option<StoredCleanupIdentity>> {
    Ok(connection
        .query_row(
            "SELECT conversation_id, agent_kind, actor_id, channel,
                    registered_session_id, actual_session_id, session_released
             FROM receiver_answer_cleanups
             WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
               AND brain_instance_id = ?4",
            rusqlite::params![
                workspace_id,
                cleanup.job_id().to_string(),
                cleanup.token().to_string(),
                cleanup.instance(),
            ],
            |row| {
                Ok(StoredCleanupIdentity {
                    conversation_id: row.get(0)?,
                    agent_kind: row.get(1)?,
                    actor_id: row.get(2)?,
                    channel: row.get(3)?,
                    registered_session_id: row.get(4)?,
                    actual_session_id: row.get(5)?,
                    session_released: row.get(6)?,
                })
            },
        )
        .optional()?)
}

fn load_cleanup(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    filter: &str,
    job_id: Option<ReceiverJobId>,
) -> Result<Option<ReceiverAnswerCleanup>> {
    let sql = format!("{} LIMIT 1", cleanup_select(filter));
    let row = job_id.map_or_else(
        || {
            connection
                .query_row(&sql, [workspace_id], load_cleanup_row)
                .optional()
        },
        |job_id| {
            connection
                .query_row(
                    &sql,
                    rusqlite::params![workspace_id, job_id.to_string()],
                    load_cleanup_row,
                )
                .optional()
        },
    )?;
    row.map(parse_cleanup_row).transpose()
}

type CleanupRow = (String, String, String, String, bool, bool, bool);

fn load_cleanup_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CleanupRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

fn cleanup_select(filter: &str) -> String {
    format!(
        "SELECT cleanup.job_id, cleanup.job_token, cleanup.brain_instance_id,
                cleanup.agent_kind, cleanup.controller_shutdown_acknowledged,
                cleanup.session_released, cleanup.artifacts_removed
         FROM receiver_answer_cleanups AS cleanup
         WHERE cleanup.workspace_id = ?1 {filter}
         ORDER BY cleanup.created_at_unix_ms, cleanup.job_id"
    )
}

fn parse_cleanup_row(row: CleanupRow) -> Result<ReceiverAnswerCleanup> {
    Ok(ReceiverAnswerCleanup::new(
        ReceiverJobId::parse(&row.0)?,
        crate::state::ReceiverJobToken::parse(&row.1)?,
        row.2,
        parse_frontend(&row.3)?,
        row.4,
        row.5,
        row.6,
    ))
}

fn controller_handoff_is_eligible(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    cleanup: &ReceiverAnswerCleanup,
) -> Result<bool> {
    if cleanup.controller_shutdown_acknowledged() {
        return Ok(true);
    }
    let state = connection
        .query_row(
            "SELECT session.locked_pid,
                    EXISTS(
                      SELECT 1 FROM brain_sessions AS replacement
                      WHERE replacement.locked_pid = session.locked_pid
                        AND replacement.brain_instance_id != session.brain_instance_id
                    )
             FROM receiver_answer_cleanups AS cleanup
             JOIN receiver_session_registrations AS registration
               ON registration.workspace_id = cleanup.workspace_id
              AND registration.conversation_id = cleanup.conversation_id
              AND registration.agent_kind = cleanup.agent_kind
              AND registration.actor_id = cleanup.actor_id
              AND registration.channel = cleanup.channel
              AND registration.brain_instance_id = cleanup.brain_instance_id
              AND registration.registered_session_id = cleanup.registered_session_id
              AND registration.actual_session_id = cleanup.actual_session_id
             JOIN brain_sessions AS session
               ON session.workspace_id = registration.workspace_id
              AND session.agent_kind = registration.agent_kind
              AND session.actor_id = registration.actor_id
              AND session.channel = registration.channel
              AND session.brain_instance_id = registration.brain_instance_id
              AND session.agent_session_id = registration.actual_session_id
             WHERE cleanup.workspace_id = ?1 AND cleanup.job_id = ?2
               AND cleanup.job_token = ?3 AND cleanup.brain_instance_id = ?4",
            rusqlite::params![
                workspace_id,
                cleanup.job_id().to_string(),
                cleanup.token().to_string(),
                cleanup.instance(),
            ],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()?;
    Ok(matches!(state, Some((None, _) | (Some(_), true))))
}

fn parse_frontend(value: &str) -> Result<crate::agent::AgentKind> {
    match value {
        "claude" => Ok(crate::agent::AgentKind::Claude),
        "codex" => Ok(crate::agent::AgentKind::Codex),
        "opencode" => Ok(crate::agent::AgentKind::OpenCode),
        _ => Err(anyhow::anyhow!("unknown receiver cleanup frontend")),
    }
}
