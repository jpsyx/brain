use anyhow::Result;
use rusqlite::Connection;

mod cleanup_schema;
mod contract;
mod downgrade;
mod fallback_success;
mod indexes;
mod legacy_notice;
mod repair;
mod structural_repair;

pub(crate) use downgrade::down_path;
#[cfg(test)]
pub(in crate::state::receiver) use downgrade::down_path_with_busy_observer;
pub(in crate::state::receiver) use structural_repair::repair_structurally_malformed_deliveries;

pub(super) fn ensure_schema(connection: &Connection) -> Result<()> {
    contract::create_table(connection)?;
    cleanup_schema::create_table(connection)?;
    contract::ensure_optional_columns(connection)?;
    cleanup_schema::ensure_optional_columns(connection)?;
    cleanup_schema::ensure_columns(connection)?;
    cleanup_schema::ensure_table_contract(connection)?;
    repair::reconcile_rows(connection)?;
    contract::ensure_table_contract(connection)?;
    legacy_notice::migrate_pending(connection)?;
    indexes::ensure_managed(connection)?;
    Ok(())
}
