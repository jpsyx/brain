//! Durable distinction between an incomplete receive and a completed conflict.
use super::Journal;
use anyhow::{Result, ensure};
use rusqlite::OptionalExtension;
use std::path::Path;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReceivePhase {
    Receiving,
    Reconcile,
}

impl Journal {
    pub(crate) fn begin_receive(&self, workspace_id: &str, started_at: &str) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS portable_receive (
            slot INTEGER PRIMARY KEY CHECK(slot = 1), workspace_id TEXT NOT NULL,
            phase TEXT NOT NULL, started_at TEXT NOT NULL);",
        )?;
        let changed = self.conn.execute("INSERT INTO portable_receive VALUES (1, ?1, 'receiving', ?2)
            ON CONFLICT(slot) DO UPDATE SET started_at=excluded.started_at
            WHERE portable_receive.workspace_id=excluded.workspace_id AND portable_receive.phase='receiving'", [workspace_id, started_at])?;
        ensure!(
            changed == 1,
            "completed portable receive requires reconciliation before another transfer"
        );
        Ok(())
    }

    pub(crate) fn complete_receive(&self, workspace_id: &str) -> Result<()> {
        ensure!(self.conn.execute("UPDATE portable_receive SET phase='reconcile' WHERE slot=1 AND workspace_id=?1 AND phase='receiving'", [workspace_id])? == 1, "portable receive completion lost its identity");
        Ok(())
    }

    pub(crate) fn receive_phase(path: &Path, workspace_id: &str) -> Result<Option<ReceivePhase>> {
        crate::sqlite_snapshot::read(path, |conn| {
        let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='portable_receive')", [], |row| row.get(0))?;
        if !exists {
            return Ok(None);
        }
        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT workspace_id, phase FROM portable_receive WHERE slot=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(id, phase)| {
            ensure!(
                id == workspace_id,
                "portable receive belongs to a different workspace"
            );
            match phase.as_str() {
                "receiving" => Ok(ReceivePhase::Receiving),
                "reconcile" => Ok(ReceivePhase::Reconcile),
                _ => anyhow::bail!("invalid portable receive phase"),
            }
        })
        .transpose()
        }).map(Option::flatten)
    }

    pub(crate) fn clear_receive(&self, workspace_id: &str) -> Result<()> {
        ensure!(self.conn.execute("DELETE FROM portable_receive WHERE slot=1 AND workspace_id=?1 AND phase='reconcile'", [workspace_id])? == 1, "portable receive reconciliation lost its identity");
        Ok(())
    }
}
