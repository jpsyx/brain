use anyhow::Result;
use rusqlite::Connection;

mod cleanup_schema;

pub(in crate::state) use cleanup_schema::{
    COLUMNS as ANSWER_CLEANUP_COLUMNS, TABLE as ANSWER_CLEANUP_TABLE,
    create_table_with as answer_cleanup_table_with,
};
mod contract;
mod cutover;
mod downgrade;
mod fallback_success;
mod indexes;
mod legacy_notice;
mod repair;
mod structural_repair;

pub(crate) use cutover::down_path as down_cutover_path;
#[cfg(test)]
pub(in crate::state::receiver) use cutover::down_path_with_busy_observer as down_cutover_path_with_busy_observer;
pub(crate) use downgrade::down_path;
#[cfg(test)]
pub(in crate::state::receiver) use downgrade::down_path_with_busy_observer;
pub(super) use legacy_notice::finish_v13_cutover;
pub(in crate::state::receiver) use structural_repair::repair_structurally_malformed_deliveries;

pub(super) fn ensure_schema(connection: &Connection) -> Result<()> {
    contract::create_table(connection)?;
    cleanup_schema::create(connection)?;
    contract::ensure_optional_columns(connection)?;
    cleanup_schema::ensure_optional_columns(connection)?;
    cleanup_schema::ensure_columns(connection)?;
    cleanup_schema::ensure_table_contract(connection)?;
    repair::reconcile_rows(connection)?;
    contract::ensure_table_contract(connection)?;
    indexes::ensure_managed(connection)?;
    Ok(())
}
