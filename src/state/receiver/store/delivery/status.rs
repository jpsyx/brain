#[cfg(test)]
use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;
#[cfg(test)]
use rusqlite::OpenFlags;

use crate::state::{Db, ReceiverDeliveryCounts};

impl Db {
    /// Return content-free durable delivery counts for diagnostics.
    pub fn receiver_delivery_counts(&self) -> Result<ReceiverDeliveryCounts> {
        receiver_delivery_counts(&self.conn)
    }

    /// Read delivery diagnostics without creating or migrating receiver state.
    #[cfg(test)]
    pub(crate) fn receiver_delivery_counts_read_only(
        path: &Path,
    ) -> Result<ReceiverDeliveryCounts> {
        if !path.is_file() {
            return Ok(ReceiverDeliveryCounts::new(0, 0, 0, 0, 0, 0));
        }
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        receiver_delivery_counts(&connection)
    }
}

fn receiver_delivery_counts(connection: &Connection) -> Result<ReceiverDeliveryCounts> {
    let table_exists: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM sqlite_master
           WHERE type = 'table' AND name = 'receiver_deliveries'
         )",
        [],
        |row| row.get(0),
    )?;
    if !table_exists {
        return Ok(ReceiverDeliveryCounts::default());
    }
    let mut counts = [0usize; 6];
    let mut statement =
        connection.prepare("SELECT state, COUNT(*) FROM receiver_deliveries GROUP BY state")?;
    for row in statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
    })? {
        let (state, count) = row?;
        match state.as_str() {
            "ready" => counts[0] = count,
            "delivering" => counts[1] = count,
            "retrying" => counts[2] = count,
            "ambiguous" => counts[3] = count,
            "failed" => counts[4] = count,
            "acknowledged" => counts[5] = count,
            _ => {}
        }
    }
    let phases = ReceiverDeliveryCounts::new(
        counts[0], counts[1], counts[2], counts[3], counts[4], counts[5],
    );
    let has_reason_columns: bool = connection.query_row(
        "SELECT
           EXISTS(SELECT 1 FROM pragma_table_info('receiver_deliveries')
                  WHERE name = 'error_category')
           AND EXISTS(SELECT 1 FROM pragma_table_info('receiver_deliveries')
                      WHERE name = 'ambiguity_reason')
           AND EXISTS(SELECT 1 FROM pragma_table_info('receiver_deliveries')
                      WHERE name = 'fallback_decision')",
        [],
        |row| row.get(0),
    )?;
    if !has_reason_columns {
        return Ok(phases);
    }
    let reasons: (usize, usize, usize, usize, usize) = connection.query_row(
        "SELECT
           COUNT(*) FILTER (
             WHERE state = 'failed' AND error_category = 'retry-exhausted'
           ),
           COUNT(*) FILTER (
             WHERE state = 'failed' AND error_category IN (
               'authorization', 'credentials', 'invalid-request', 'provider-rejected'
             )
           ),
           COUNT(*) FILTER (
             WHERE state = 'ambiguous' AND ambiguity_reason IN (
               'provider-acceptance-unknown', 'provider-acknowledgement-malformed',
               'result-commit-unknown'
             )
           ),
           COUNT(*) FILTER (
             WHERE (state = 'ambiguous'
                    AND ambiguity_reason = 'idempotency-window-expired')
                OR (state = 'failed'
                    AND error_category = 'idempotency-window-expired')
           ),
           COUNT(*) FILTER (WHERE fallback_decision = 'no-safe-fallback')
         FROM receiver_deliveries",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    Ok(phases.with_terminal_reasons(reasons.0, reasons.1, reasons.2, reasons.3, reasons.4))
}
