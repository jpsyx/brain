use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension as _};

pub(super) fn ensure_managed(connection: &Connection) -> Result<()> {
    if !matches(
        connection,
        "receiver_deliveries_job_kind",
        true,
        &["job_id", "response_kind"],
    )? {
        reject_duplicate_semantic_responses(connection)?;
        connection.execute_batch(
            "DROP INDEX IF EXISTS receiver_deliveries_job_kind;
             CREATE UNIQUE INDEX receiver_deliveries_job_kind
               ON receiver_deliveries(job_id, response_kind);",
        )?;
    }
    if !matches(
        connection,
        "receiver_deliveries_due",
        false,
        &[
            "state",
            "retry_at_unix_ms",
            "created_at_unix_ms",
            "delivery_id",
        ],
    )? {
        connection.execute_batch(
            "DROP INDEX IF EXISTS receiver_deliveries_due;
             CREATE INDEX receiver_deliveries_due
               ON receiver_deliveries(
                 state, retry_at_unix_ms, created_at_unix_ms, delivery_id
               );",
        )?;
    }
    Ok(())
}

fn matches(
    connection: &Connection,
    index_name: &str,
    expected_unique: bool,
    expected_columns: &[&str],
) -> Result<bool> {
    let unique = connection
        .query_row(
            "SELECT \"unique\" FROM pragma_index_list('receiver_deliveries')
             WHERE name = ?1",
            [index_name],
            |row| row.get::<_, bool>(0),
        )
        .optional()?;
    let Some(unique) = unique else {
        return Ok(false);
    };
    let mut statement =
        connection.prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")?;
    let columns = statement
        .query_map([index_name], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(unique == expected_unique
        && columns
            .iter()
            .map(String::as_str)
            .eq(expected_columns.iter().copied()))
}

pub(super) fn reject_duplicate_semantic_responses(connection: &Connection) -> Result<()> {
    let has_duplicates: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM receiver_deliveries
           GROUP BY job_id, response_kind HAVING COUNT(*) > 1
         )",
        [],
        |row| row.get(0),
    )?;
    if has_duplicates {
        bail!("receiver delivery schema contains duplicate semantic responses");
    }
    Ok(())
}
