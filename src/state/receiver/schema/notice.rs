use anyhow::Result;
use rusqlite::Connection;

use super::{RECOVERY_VERSION, has_column};
use crate::state::Db;

pub(super) fn ensure_unavailable_notice_columns(connection: &Connection) -> Result<()> {
    if !has_column(connection, "unavailable_notice_owner")? {
        connection
            .execute_batch("ALTER TABLE receiver_jobs ADD COLUMN unavailable_notice_owner TEXT;")?;
    }
    if !has_column(connection, "unavailable_notice_expires_at_unix_ms")? {
        connection.execute_batch(
            "ALTER TABLE receiver_jobs
             ADD COLUMN unavailable_notice_expires_at_unix_ms INTEGER;",
        )?;
    }
    connection.execute_batch(
        "UPDATE receiver_jobs
         SET unavailable_notice_owner = NULL,
             unavailable_notice_expires_at_unix_ms = NULL
         WHERE (unavailable_notice_owner IS NULL)
            != (unavailable_notice_expires_at_unix_ms IS NULL);",
    )?;
    Ok(())
}

pub(crate) fn down_unavailable_notice_path(path: &std::path::Path) -> Result<()> {
    down_unavailable_notice_path_inner(path, None)
}

#[cfg(test)]
pub(in crate::state::receiver) fn down_unavailable_notice_path_with_busy_observer(
    path: &std::path::Path,
    observer: fn(i32) -> bool,
) -> Result<()> {
    down_unavailable_notice_path_inner(path, Some(observer))
}

fn down_unavailable_notice_path_inner(
    path: &std::path::Path,
    busy_observer: Option<fn(i32) -> bool>,
) -> Result<()> {
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
    let has_owner = has_column(&transaction, "unavailable_notice_owner")?;
    let has_expiry = has_column(&transaction, "unavailable_notice_expires_at_unix_ms")?;
    if has_owner {
        transaction
            .execute_batch("ALTER TABLE receiver_jobs DROP COLUMN unavailable_notice_owner;")?;
    }
    if has_expiry {
        transaction.execute_batch(
            "ALTER TABLE receiver_jobs DROP COLUMN unavailable_notice_expires_at_unix_ms;",
        )?;
    }
    transaction.pragma_update(None, "user_version", RECOVERY_VERSION)?;
    transaction.commit()?;
    Ok(())
}
