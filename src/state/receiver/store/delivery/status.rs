use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, OpenFlags};

use crate::state::{Db, ReceiverDeliveryCounts};

impl Db {
    /// Return content-free durable delivery counts for diagnostics.
    pub fn receiver_delivery_counts(&self) -> Result<ReceiverDeliveryCounts> {
        receiver_delivery_counts(&self.conn)
    }

    /// Read delivery diagnostics without creating or migrating receiver state.
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
    Ok(ReceiverDeliveryCounts::new(
        counts[0], counts[1], counts[2], counts[3], counts[4], counts[5],
    ))
}
