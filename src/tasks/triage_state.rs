//! Whether this month's weekly triage has already happened.
//!
//! "Monthly triage" is not a separate pass: it is the **first weekly triage of
//! a calendar month**. That is one bit of state, and it lives here rather than
//! in a skill's head so the answer survives a session ending mid-run.

use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    last_monthly_triage_month: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct TriageState {
    /// True when this month has not had its monthly pass yet.
    pub(crate) is_monthly: bool,
    /// The current month, `YYYY-MM`.
    pub(crate) month: String,
    /// The last month recorded, or empty.
    pub(crate) last_recorded: String,
}

pub(crate) fn state_path(root: &Path) -> PathBuf {
    root.join("tasks/.monthly_triage.json")
}

/// `YYYY-MM` for `date`.
pub(crate) fn month_of(date: NaiveDate) -> String {
    format!("{:04}-{:02}", date.year(), date.month())
}

fn load(root: &Path) -> String {
    std::fs::read_to_string(state_path(root))
        .ok()
        .and_then(|text| serde_json::from_str::<Stored>(&text).ok())
        .map(|stored| stored.last_monthly_triage_month)
        .unwrap_or_default()
}

pub(crate) fn read(root: &Path, today: NaiveDate) -> TriageState {
    let month = month_of(today);
    let last_recorded = load(root);
    TriageState {
        is_monthly: last_recorded != month,
        month,
        last_recorded,
    }
}

/// Record this month as having had its monthly pass. Returns the previous mark.
pub(crate) fn mark(root: &Path, today: NaiveDate) -> Result<String> {
    let previous = load(root);
    let month = month_of(today);
    let path = state_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        &path,
        serde_json::to_string(&Stored {
            last_monthly_triage_month: month,
        })? + "\n",
    )?;
    Ok(previous)
}

#[cfg(test)]
mod tests {
    use super::{mark, month_of, read};
    use chrono::NaiveDate;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    #[test]
    fn a_month_is_zero_padded() {
        assert_eq!(month_of(date(2026, 3, 9)), "2026-03");
    }

    #[test]
    fn an_unmarked_month_is_the_monthly_one() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let state = read(temporary.path(), date(2026, 8, 24));
        assert!(state.is_monthly);
        assert_eq!(state.month, "2026-08");
        assert!(state.last_recorded.is_empty());
    }

    #[test]
    fn marking_makes_the_rest_of_the_month_ordinary() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let root = temporary.path();

        let previous = mark(root, date(2026, 8, 24)).expect("mark");

        assert!(previous.is_empty());
        assert!(!read(root, date(2026, 8, 31)).is_monthly);
        // …and the next month starts over.
        assert!(read(root, date(2026, 9, 1)).is_monthly);
    }

    #[test]
    fn an_unreadable_state_file_reads_as_never_marked() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let root = temporary.path();
        std::fs::create_dir_all(root.join("tasks")).expect("tasks dir");
        std::fs::write(super::state_path(root), "not json").expect("write");

        assert!(read(root, date(2026, 8, 24)).is_monthly);
    }
}
