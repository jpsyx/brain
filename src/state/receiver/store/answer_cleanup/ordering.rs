use anyhow::Result;

use crate::state::ReceiverAnswerCleanup;

pub(super) fn defer(
    connection: &rusqlite::Connection,
    workspace_id: &str,
    cleanup: &ReceiverAnswerCleanup,
    observed_at_unix_ms: i64,
) -> Result<bool> {
    let transaction =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)?;
    let maximum = transaction.query_row(
        "SELECT MAX(peer.updated_at_unix_ms)
             FROM receiver_answer_cleanups AS peer
             WHERE peer.workspace_id = ?1
               AND EXISTS (
                 SELECT 1 FROM receiver_answer_cleanups AS target
                 WHERE target.workspace_id = ?1 AND target.job_id = ?2
                   AND target.job_token = ?3 AND target.brain_instance_id = ?4
                   AND target.controller_shutdown_acknowledged = 1
               )",
        rusqlite::params![
            workspace_id,
            cleanup.job_id().to_string(),
            cleanup.token().to_string(),
            cleanup.instance(),
        ],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    let Some(maximum) = maximum else {
        return Ok(false);
    };
    if maximum == i64::MAX {
        transaction.execute(
            "UPDATE receiver_answer_cleanups
             SET updated_at_unix_ms = CASE
               WHEN updated_at_unix_ms = ?5 THEN ?5
               ELSE updated_at_unix_ms - 1
             END
             WHERE workspace_id = ?1
               AND NOT (job_id = ?2 AND job_token = ?3 AND brain_instance_id = ?4)",
            rusqlite::params![
                workspace_id,
                cleanup.job_id().to_string(),
                cleanup.token().to_string(),
                cleanup.instance(),
                i64::MIN,
            ],
        )?;
    }
    let deferred_at = if maximum == i64::MAX {
        i64::MAX
    } else {
        observed_at_unix_ms.max(maximum + 1)
    };
    let changed = transaction.execute(
        "UPDATE receiver_answer_cleanups
         SET updated_at_unix_ms = ?5
         WHERE workspace_id = ?1 AND job_id = ?2 AND job_token = ?3
           AND brain_instance_id = ?4 AND controller_shutdown_acknowledged = 1",
        rusqlite::params![
            workspace_id,
            cleanup.job_id().to_string(),
            cleanup.token().to_string(),
            cleanup.instance(),
            deferred_at,
        ],
    )?;
    transaction.commit()?;
    Ok(changed == 1)
}
