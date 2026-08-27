//! Workspace-wide live receiver-claim exclusion.

use anyhow::Result;

pub(super) fn has_live_receiver_claim(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
    now: i64,
) -> Result<bool> {
    Ok(transaction.query_row(
        "SELECT EXISTS (
           SELECT 1 FROM receiver_jobs
           WHERE workspace_id = ?1
             AND claim_owner IS NOT NULL
             AND claim_expires_at_unix_ms > ?2
             AND state NOT IN ('failed', 'done')
         )",
        rusqlite::params![workspace_id, now],
        |row| row.get(0),
    )?)
}
