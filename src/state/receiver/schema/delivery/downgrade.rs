use anyhow::{Context as _, Result, bail};
use rusqlite::Connection;

use super::super::{DELIVERY_PREVIOUS_VERSION, VERSION};
use crate::state::Db;

const REQUIRED_CONVERSATION_COLUMNS: &[&str] = &[
    "conversation_id",
    "workspace_id",
    "user_id",
    "channel",
    "conversation_key",
    "transcript_markdown",
    "agent_kind",
    "agent_session_id",
    "created_at_unix_ms",
    "updated_at_unix_ms",
];

const REQUIRED_JOB_COLUMNS: &[&str] = &[
    "job_id",
    "job_token",
    "workspace_id",
    "conversation_id",
    "channel",
    "provider_id",
    "inbound_json",
    "state",
    "received_at_unix_ms",
    "updated_at_unix_ms",
    "claim_owner",
    "claim_expires_at_unix_ms",
    "retry_count",
    "retry_at_unix_ms",
    "retry_from_state",
    "last_error",
    "launched_at_unix_ms",
    "accepted_at_unix_ms",
    "progressing_at_unix_ms",
    "completed_at_unix_ms",
    "observation_instance",
    "observation_session_id",
    "observation_revision",
    "attempt_accepted_at_unix_ms",
    "attempt_progressing_at_unix_ms",
    "latest_progress_at_unix_ms",
    "launch_expires_at_unix_ms",
    "acceptance_expires_at_unix_ms",
    "progress_expires_at_unix_ms",
    "recovery_expires_at_unix_ms",
    "absolute_work_expires_at_unix_ms",
    "recovery_count",
    "attempt_kind",
    "pending_unavailable_notice",
    "recovery_cleanup_instance",
    "recovery_cleanup_session_id",
    "unavailable_notice_owner",
    "unavailable_notice_expires_at_unix_ms",
];

const REQUIRED_REGISTRATION_COLUMNS: &[&str] = &[
    "workspace_id",
    "conversation_id",
    "agent_kind",
    "actor_id",
    "channel",
    "brain_instance_id",
    "registered_session_id",
    "actual_session_id",
];

struct CleanupObligation {
    workspace_id: String,
    conversation_id: String,
    instance: String,
    agent_kind: String,
    actor_id: String,
    channel: String,
    registered_session_id: String,
    actual_session_id: String,
    controller_shutdown_acknowledged: bool,
    session_released: bool,
}

pub(crate) fn down_path(path: &std::path::Path) -> Result<()> {
    down_path_inner(path, None)
}

#[cfg(test)]
pub(in crate::state::receiver) fn down_path_with_busy_observer(
    path: &std::path::Path,
    observer: fn(i32) -> bool,
) -> Result<()> {
    down_path_inner(path, Some(observer))
}

fn down_path_inner(path: &std::path::Path, busy_observer: Option<fn(i32) -> bool>) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let connection = Connection::open(path)?;
    Db::configure(&connection)?;
    if let Some(observer) = busy_observer {
        connection.busy_handler(Some(observer))?;
    }
    let transaction = rusqlite::Transaction::new_unchecked(
        &connection,
        rusqlite::TransactionBehavior::Immediate,
    )?;
    let version: i32 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != VERSION {
        transaction.commit()?;
        return Ok(());
    }
    validate_previous_schema(&transaction)?;
    drain_answer_cleanups(&transaction, path)?;
    if table_exists(&transaction, "receiver_deliveries")? {
        transaction.execute_batch(
            "UPDATE receiver_jobs
             SET state = 'done', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 pending_unavailable_notice = 0,
                 unavailable_notice_owner = NULL,
                 unavailable_notice_expires_at_unix_ms = NULL
             WHERE EXISTS (
               SELECT 1 FROM receiver_deliveries AS delivery
               WHERE delivery.job_id = receiver_jobs.job_id
             )
             AND NOT EXISTS (
               SELECT 1 FROM receiver_deliveries AS delivery
               WHERE delivery.job_id = receiver_jobs.job_id
                 AND (delivery.state != 'acknowledged'
                   OR delivery.provider_reference IS NULL
                   OR length(trim(delivery.provider_reference)) = 0)
             );
             UPDATE receiver_jobs
             SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 last_error = 'downgrade-no-replay', pending_unavailable_notice = 0,
                 unavailable_notice_owner = NULL,
                 unavailable_notice_expires_at_unix_ms = NULL
             WHERE EXISTS (
               SELECT 1 FROM receiver_deliveries AS delivery
               WHERE delivery.job_id = receiver_jobs.job_id
                 AND (delivery.state != 'acknowledged'
                   OR delivery.provider_reference IS NULL
                   OR length(trim(delivery.provider_reference)) = 0)
             );
             UPDATE receiver_jobs
             SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 last_error = 'downgrade-no-replay', pending_unavailable_notice = 0,
                 unavailable_notice_owner = NULL,
                 unavailable_notice_expires_at_unix_ms = NULL
             WHERE state IN ('answer-ready', 'delivering', 'retrying')
               AND (state != 'retrying' OR retry_from_state IS NULL)
               AND NOT EXISTS (
                 SELECT 1 FROM receiver_deliveries AS delivery
                 WHERE delivery.job_id = receiver_jobs.job_id
                   AND delivery.job_token = receiver_jobs.job_token
               );",
        )?;
        super::fallback_success::restore_acknowledged_jobs(&transaction)?;
    } else {
        transaction.execute_batch(
            "UPDATE receiver_jobs
             SET state = 'failed', claim_owner = NULL, claim_expires_at_unix_ms = NULL,
                 retry_at_unix_ms = NULL, retry_from_state = NULL,
                 last_error = 'downgrade-no-replay', pending_unavailable_notice = 0,
                 unavailable_notice_owner = NULL,
                 unavailable_notice_expires_at_unix_ms = NULL
             WHERE state IN ('answer-ready', 'delivering', 'retrying')
               AND (state != 'retrying' OR retry_from_state IS NULL);",
        )?;
    }
    transaction.execute_batch(
        "DROP TABLE IF EXISTS receiver_answer_cleanups;
         DROP INDEX IF EXISTS receiver_deliveries_due;
         DROP INDEX IF EXISTS receiver_deliveries_job_kind;
         DROP TABLE IF EXISTS receiver_deliveries;",
    )?;
    super::super::job_contract::rebuild_exact_v11(&transaction)?;
    transaction.pragma_update(None, "user_version", DELIVERY_PREVIOUS_VERSION)?;
    transaction.commit()?;
    Ok(())
}

fn drain_answer_cleanups(connection: &Connection, state_path: &std::path::Path) -> Result<()> {
    if !table_exists(connection, "receiver_answer_cleanups")? {
        bail!("receiver v12 schema is missing answer cleanup authority");
    }
    let cleanups = load_cleanup_obligations(connection)?;
    for cleanup in &cleanups {
        if !cleanup.controller_shutdown_acknowledged {
            bail!("receiver answer cleanup lacks confirmed controller shutdown");
        }
        validate_artifact_instance(&cleanup.instance)?;
        if !cleanup.session_released && !exact_session_registration_exists(connection, cleanup)? {
            bail!("receiver answer cleanup lacks its exact session authority");
        }
    }
    let cache_dir = state_path
        .parent()
        .context("receiver state path has no workspace cache directory")?;
    for cleanup in &cleanups {
        remove_cleanup_artifacts(cache_dir, &cleanup.instance)?;
    }
    for cleanup in &cleanups {
        if cleanup.session_released {
            continue;
        }
        connection.execute(
            "UPDATE brain_sessions
             SET locked_pid = NULL
             WHERE workspace_id = ?1 AND agent_kind = ?2 AND actor_id = ?3
               AND channel = ?4 AND brain_instance_id = ?5
               AND agent_session_id = ?6",
            rusqlite::params![
                cleanup.workspace_id,
                cleanup.agent_kind,
                cleanup.actor_id,
                cleanup.channel,
                cleanup.instance,
                cleanup.actual_session_id,
            ],
        )?;
        let deleted = connection.execute(
            "DELETE FROM receiver_session_registrations
             WHERE workspace_id = ?1 AND conversation_id = ?2
               AND agent_kind = ?3 AND actor_id = ?4 AND channel = ?5
               AND brain_instance_id = ?6 AND registered_session_id = ?7
               AND actual_session_id = ?8",
            rusqlite::params![
                cleanup.workspace_id,
                cleanup.conversation_id,
                cleanup.agent_kind,
                cleanup.actor_id,
                cleanup.channel,
                cleanup.instance,
                cleanup.registered_session_id,
                cleanup.actual_session_id,
            ],
        )?;
        if deleted != 1 {
            bail!("receiver answer cleanup exact session changed during downgrade");
        }
    }
    connection.execute("DELETE FROM receiver_answer_cleanups", [])?;
    Ok(())
}

fn load_cleanup_obligations(connection: &Connection) -> Result<Vec<CleanupObligation>> {
    let mut statement = connection.prepare(
        "SELECT workspace_id, conversation_id, brain_instance_id, agent_kind,
                actor_id, channel, registered_session_id, actual_session_id,
                controller_shutdown_acknowledged, session_released
         FROM receiver_answer_cleanups
         ORDER BY created_at_unix_ms, job_id",
    )?;
    let cleanups = statement
        .query_map([], |row| {
            Ok(CleanupObligation {
                workspace_id: row.get(0)?,
                conversation_id: row.get(1)?,
                instance: row.get(2)?,
                agent_kind: row.get(3)?,
                actor_id: row.get(4)?,
                channel: row.get(5)?,
                registered_session_id: row.get(6)?,
                actual_session_id: row.get(7)?,
                controller_shutdown_acknowledged: row.get(8)?,
                session_released: row.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(cleanups)
}

fn exact_session_registration_exists(
    connection: &Connection,
    cleanup: &CleanupObligation,
) -> Result<bool> {
    Ok(connection.query_row(
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
            cleanup.workspace_id,
            cleanup.conversation_id,
            cleanup.agent_kind,
            cleanup.actor_id,
            cleanup.channel,
            cleanup.instance,
            cleanup.registered_session_id,
            cleanup.actual_session_id,
        ],
        |row| row.get(0),
    )?)
}

fn validate_artifact_instance(instance: &str) -> Result<()> {
    if instance.is_empty()
        || instance.len() > 128
        || !instance
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("receiver answer cleanup has an unsafe artifact identity");
    }
    Ok(())
}

fn remove_cleanup_artifacts(cache_dir: &std::path::Path, instance: &str) -> Result<()> {
    let response = cache_dir.join("responses").join(format!("{instance}.json"));
    let observation = cache_dir
        .join("receiver-observations")
        .join(format!("{instance}.json"));
    for artifact in [
        response,
        observation.clone(),
        observation.with_extension("json.lock"),
    ] {
        match std::fs::remove_file(&artifact) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("remove receiver cleanup artifact {}", artifact.display())
                });
            }
        }
    }
    Ok(())
}

fn validate_previous_schema(connection: &Connection) -> Result<()> {
    validate_table_columns(
        connection,
        "receiver_conversations",
        REQUIRED_CONVERSATION_COLUMNS,
    )?;
    validate_table_columns(connection, "receiver_jobs", REQUIRED_JOB_COLUMNS)?;
    validate_table_columns(
        connection,
        "receiver_session_registrations",
        REQUIRED_REGISTRATION_COLUMNS,
    )?;
    Ok(())
}

fn validate_table_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> Result<()> {
    if !table_exists(connection, table)? {
        bail!("receiver v11 schema is missing required table {table}");
    }
    for column in required_columns {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
            rusqlite::params![table, column],
            |row| row.get(0),
        )?;
        if !exists {
            bail!("receiver v11 schema is missing required {table} column {column}");
        }
    }
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
         )",
        [table],
        |row| row.get(0),
    )?)
}
