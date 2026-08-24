//! `brain tasks assign <id> <user>` — hand a task to another workspace member.
//!
//! The user ID must name a real portable member: an assignment to a stranger is
//! a task nobody owns, which is worse than one assigned to the wrong person.

use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;

use super::locate_target;
use crate::tasks::agenda::{Action, Outcome, Targets, sync_targets};
use crate::tasks::complete::{field, touch_row, write_csv};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssignResult {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) previous: String,
    pub(crate) assigned_to: String,
}

/// Validate `user_id` against the workspace's portable roster.
pub(crate) fn validate_member(root: &Path, user_id: &str) -> Result<String> {
    let id = crate::users::UserId::parse(user_id).map_err(anyhow::Error::msg)?;
    let users = crate::users::UsersStore::load_from(&root.join(".config/users.json"))
        .with_context(|| {
            format!(
                "cannot validate assigned_to without {}/.config/users.json",
                root.display()
            )
        })?;
    if users.user(&id).is_none() {
        bail!("assigned_to '{user_id}' is not a workspace member");
    }
    Ok(user_id.to_owned())
}

pub(crate) fn assign_in_root(
    root: &Path,
    targets: &Targets,
    raw_id: &str,
    user_id: &str,
    today: NaiveDate,
) -> Result<(AssignResult, Outcome)> {
    let assigned_to = validate_member(root, user_id)?;
    let mut target = locate_target(root, raw_id)?;
    target.ensure_column("assigned_to");
    target.ensure_column("last_touched");
    let today_string = today.to_string();
    let row = target.row_mut()?;
    let previous = field(row, "assigned_to");
    row.insert("assigned_to".to_owned(), assigned_to.clone());
    touch_row(row, &today_string);
    let result = AssignResult {
        task_id: field(row, "task_id"),
        task_name: field(row, "task_name"),
        previous,
        assigned_to,
    };
    write_csv(&target.path, &target.csv)?;

    let outcome = sync_targets(targets, &result.task_id, Action::Touch, today);
    Ok((result, outcome))
}
