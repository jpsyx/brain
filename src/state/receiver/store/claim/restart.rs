use anyhow::Result;

use crate::server::receiver::{ControlCommand, InboundJob, parse_control_command};

pub(super) fn has_ready_restart(
    transaction: &rusqlite::Transaction<'_>,
    workspace_id: &str,
) -> Result<bool> {
    let mut statement = transaction.prepare(
        "SELECT inbound_json FROM receiver_jobs
         WHERE workspace_id = ?1 AND state = 'queued' AND claim_owner IS NULL
         ORDER BY received_at_unix_ms, job_id",
    )?;
    let inbound = statement.query_map([workspace_id], |row| row.get::<_, String>(0))?;
    for inbound in inbound {
        let inbound: InboundJob = serde_json::from_str(&inbound?)?;
        if parse_control_command(&inbound.prompt) == Some(ControlCommand::Restart) {
            return Ok(true);
        }
    }
    Ok(false)
}
