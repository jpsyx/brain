//! One source of truth for the `agent_kind` column contract.
//!
//! Every table that stores a frontend name constrains it to the registered set,
//! and that set grows when Brain learns a new frontend. A database created
//! before the new frontend still carries the old `CHECK`, and
//! `CREATE TABLE IF NOT EXISTS` never repairs one, so each such table is
//! rebuilt when its stored definition no longer matches the current contract.

use anyhow::Result;
use rusqlite::Connection;

use crate::agent::AgentKind;

/// The quoted, comma-separated frontend names for a `CHECK ... IN (...)` list,
/// in registry order.
pub(super) fn agent_kind_values() -> String {
    AgentKind::ALL
        .iter()
        .map(|kind| format!("'{}'", kind.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether a table's stored definition already is `canonical_create`.
///
/// Returns `false` for a table that does not exist yet: the caller's
/// `CREATE TABLE IF NOT EXISTS` has already run by then, so a missing row means
/// something is wrong rather than something to rebuild.
pub(super) fn table_contract_matches(
    connection: &Connection,
    table: &str,
    canonical_create: &str,
) -> bool {
    let stored: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .ok();
    let canonical = canonical_create.replacen("CREATE TABLE IF NOT EXISTS", "CREATE TABLE", 1);
    stored.is_some_and(|stored| normalize(&stored) == normalize(&canonical))
}

/// Rebuild `table` to `canonical_create`, carrying `columns` across and
/// dropping any row whose `agent_kind` the new contract does not allow.
///
/// Dropping is the honest choice in both directions: a row naming a frontend
/// this Brain does not have cannot be launched, resumed, or answered.
pub(super) fn rebuild_with_agent_kind_contract(
    connection: &Connection,
    table: &str,
    canonical_create: &str,
    columns: &str,
    allowed_agent_kinds: &str,
) -> Result<()> {
    let staging = format!("{table}_agent_kind_rebuild");
    connection.execute_batch(&format!("ALTER TABLE {table} RENAME TO {staging};"))?;
    connection.execute_batch(canonical_create)?;
    connection.execute_batch(&format!(
        "INSERT INTO {table} ({columns})
           SELECT {columns} FROM {staging}
           WHERE agent_kind IN ({allowed_agent_kinds});
         DROP TABLE {staging};"
    ))?;
    Ok(())
}

/// Bring one table's stored `agent_kind` contract up to the current frontend
/// registry, rebuilding only when it has drifted.
pub(super) fn ensure_agent_kind_contract(
    connection: &Connection,
    table: &str,
    canonical_create: &str,
    columns: &str,
) -> Result<()> {
    if table_contract_matches(connection, table, canonical_create) {
        return Ok(());
    }
    rebuild_with_agent_kind_contract(
        connection,
        table,
        canonical_create,
        columns,
        &agent_kind_values(),
    )
}

/// Frontends a Brain older than the pi release knows.
const PRE_PI_AGENT_KINDS: &str = "'claude', 'codex', 'opencode'";

/// Restore every `agent_kind` contract to the pre-pi frontend set.
///
/// Rows naming a frontend the older Brain cannot launch are dropped: leaving
/// one behind would make the older binary reject the whole table.
pub(crate) fn down_path(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let connection = rusqlite::Connection::open(path)?;
    crate::state::Db::configure(&connection)?;
    // SQLite's own recipe for a table rebuild: rename, recreate, copy. Both
    // pragmas keep the copy from depending on tables this database may not have,
    // because references to the renamed name are otherwise rewritten and the
    // copy's foreign keys are otherwise resolved mid-rebuild.
    connection.pragma_update(None, "legacy_alter_table", true)?;
    connection.pragma_update(None, "foreign_keys", false)?;
    let result = rebuild_to_previous_contract(&connection);
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "legacy_alter_table", false)?;
    result
}

fn rebuild_to_previous_contract(connection: &Connection) -> Result<()> {
    let transaction = rusqlite::Transaction::new_unchecked(
        connection,
        rusqlite::TransactionBehavior::Immediate,
    )?;
    for (table, columns, create) in [
        (
            super::manual_session::schema::TABLE,
            super::manual_session::schema::COLUMNS,
            super::manual_session::schema::create_table_with(PRE_PI_AGENT_KINDS),
        ),
        (
            super::receiver::schema::REGISTRATION_TABLE,
            super::receiver::schema::REGISTRATION_COLUMNS,
            super::receiver::schema::registration_table_with(PRE_PI_AGENT_KINDS),
        ),
        (
            super::receiver::schema::answer_cleanup_table(),
            super::receiver::schema::answer_cleanup_columns(),
            super::receiver::schema::answer_cleanup_table_with(PRE_PI_AGENT_KINDS),
        ),
    ] {
        if table_exists(&transaction, table)
            && !table_contract_matches(&transaction, table, &create)
        {
            rebuild_with_agent_kind_contract(
                &transaction,
                table,
                &create,
                columns,
                PRE_PI_AGENT_KINDS,
            )?;
        }
    }
    if table_exists(&transaction, super::manual_session::schema::TABLE) {
        transaction.execute_batch(super::manual_session::schema::ONE_MAIN_INDEX)?;
    }
    transaction.commit()?;
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> bool {
    connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |_| Ok(()),
        )
        .is_ok()
}

fn normalize(sql: &str) -> String {
    sql.trim()
        .trim_end_matches(';')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_list_is_the_frontend_registry_in_order() {
        assert_eq!(agent_kind_values(), "'claude', 'codex', 'opencode', 'pi'");
    }
}
