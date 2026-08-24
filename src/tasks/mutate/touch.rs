//! `brain tasks touch <id>` — bump `last_touched` and nothing else.
//!
//! This is the chronic-ignore "yes, I still care" acknowledgement: the row is
//! not done, not deferred, not demoted. It just stops being stale, which buys
//! it another quiet window before the scan flags it again.

use std::path::Path;

use anyhow::Result;
use chrono::NaiveDate;

use super::locate_target;
use crate::tasks::agenda::{Action, Outcome, Targets, sync_targets};
use crate::tasks::complete::{field, touch_row, write_csv};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TouchResult {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    /// The previous `last_touched`, or `(never)`.
    pub(crate) previous: String,
}

pub(crate) fn touch_in_root(
    root: &Path,
    targets: &Targets,
    raw_id: &str,
    today: NaiveDate,
) -> Result<(TouchResult, Outcome)> {
    let mut target = locate_target(root, raw_id)?;
    target.ensure_column("last_touched");
    let today_string = today.to_string();
    let row = target.row_mut()?;
    let previous = field(row, "last_touched");
    let previous = if previous.trim().is_empty() {
        "(never)".to_owned()
    } else {
        previous
    };
    touch_row(row, &today_string);
    let result = TouchResult {
        task_id: field(row, "task_id"),
        task_name: field(row, "task_name"),
        previous,
    };
    write_csv(&target.path, &target.csv)?;

    let outcome = sync_targets(targets, &result.task_id, Action::Touch, today);
    Ok((result, outcome))
}
