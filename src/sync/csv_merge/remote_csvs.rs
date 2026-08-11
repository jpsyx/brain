//! Classifying a remote's task CSVs by what they *contain*.
//!
//! Whether remote CSV files exist says nothing about whether they hold legacy
//! rows, but existence was used as the proxy for exactly that. A remote holding
//! header-only current CSVs was therefore treated as legacy data to protect,
//! which refused both the plain sync and the setup that would have fixed it.

use anyhow::Result;

/// What the remote's task CSVs prove about its schema generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteCsvState {
    /// No task CSV content on the remote at all.
    Absent,
    /// Every CSV present carries the current row identity.
    Current,
    /// At least one CSV present predates the current row identity.
    Legacy,
}

/// Classify the remote's task CSVs. Pure.
///
/// An empty or whitespace-only object is content-free rather than legacy: it
/// proves nothing about a schema generation, so it must not veto initialization.
pub fn classify_remote_csvs(tasks: Option<&str>, habits: Option<&str>) -> Result<RemoteCsvState> {
    let mut present = false;
    for text in [tasks, habits].into_iter().flatten() {
        if text.trim().is_empty() {
            continue;
        }
        present = true;
        if !crate::tasks::schema::csv_has_current_identity(text.as_bytes())? {
            return Ok(RemoteCsvState::Legacy);
        }
    }
    Ok(if present {
        RemoteCsvState::Current
    } else {
        RemoteCsvState::Absent
    })
}

#[cfg(test)]
mod tests {
    use super::{RemoteCsvState, classify_remote_csvs};

    const CURRENT_TASKS: &str = "task_uuid,task_id,task_name,assigned_to,system_key\n";
    const CURRENT_HABITS: &str = "task_uuid,task_id,task_name,assigned_to,system_key\n";
    const LEGACY_TASKS: &str = "task_id,status\nT1,open\n";

    #[test]
    fn nothing_on_the_remote_is_absent_not_legacy() {
        assert_eq!(
            classify_remote_csvs(None, None).unwrap(),
            RemoteCsvState::Absent
        );
    }

    /// The `~/family` shape: seeded header-only CSVs already on the remote.
    #[test]
    fn header_only_current_csvs_are_current() {
        assert_eq!(
            classify_remote_csvs(Some(CURRENT_TASKS), Some(CURRENT_HABITS)).unwrap(),
            RemoteCsvState::Current
        );
    }

    #[test]
    fn an_empty_object_proves_nothing_and_is_absent() {
        assert_eq!(
            classify_remote_csvs(Some("   \n"), Some("")).unwrap(),
            RemoteCsvState::Absent
        );
    }

    #[test]
    fn one_legacy_csv_makes_the_remote_legacy() {
        assert_eq!(
            classify_remote_csvs(Some(LEGACY_TASKS), Some(CURRENT_HABITS)).unwrap(),
            RemoteCsvState::Legacy
        );
        assert_eq!(
            classify_remote_csvs(Some(CURRENT_TASKS), Some(LEGACY_TASKS)).unwrap(),
            RemoteCsvState::Legacy
        );
    }

    #[test]
    fn a_current_header_with_an_unparsable_uuid_row_is_legacy() {
        let rows = "task_uuid,task_id,task_name,assigned_to,system_key\n,T1,thing,,\n";
        assert_eq!(
            classify_remote_csvs(Some(rows), None).unwrap(),
            RemoteCsvState::Legacy
        );
    }
}
