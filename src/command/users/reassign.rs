//! Atomic reassignment of any task assignment value onto a portable member.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::tasks::complete::{parse_csv_bytes, serialize_csv};
use crate::users::{FileChange, UserId, Users, UsersStore, replace_group};
use crate::workspace::WorkspaceContext;

/// One CSV's bytes before and after a rewrite, plus how many rows moved.
type RewrittenCsv = (Vec<u8>, Vec<u8>, usize);

/// Every distinct assignment value that names nobody in the portable registry.
pub(super) fn unmapped_assignments(values: &[String], users: &Users) -> Vec<String> {
    let mut unmapped: Vec<String> = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || unmapped.iter().any(|seen| seen == value) {
            continue;
        }
        let mapped = UserId::parse(value)
            .ok()
            .is_some_and(|id| users.user(&id).is_some());
        if !mapped {
            unmapped.push(value.to_owned());
        }
    }
    unmapped
}

/// Move every row assigned to one raw value onto an existing portable member.
///
/// Returns how many rows moved. Nothing is written when nothing matches.
pub(super) fn reassign(workspace: &WorkspaceContext, from: &str, to: &UserId) -> Result<usize> {
    let _task_owner = crate::tasks::store_lock::TaskStoreOwner::acquire(workspace)?;
    let users = UsersStore::load(workspace)?;
    if users.user(to).is_none() {
        return Err(crate::users::UsersError::UnknownUser {
            user_id: to.to_string(),
        }
        .into());
    }
    let from = from.trim();
    if from.is_empty() {
        anyhow::bail!("the assignment value to move cannot be empty");
    }

    let mut changes = Vec::new();
    let mut moved = 0;
    for path in assignment_paths(workspace) {
        let Some((before, after, rows)) = rewritten_csv(&path, from, to)? else {
            continue;
        };
        moved += rows;
        if before != after {
            changes.push(FileChange::new(path, before, after));
        }
    }
    if changes.is_empty() {
        return Ok(moved);
    }
    replace_group(
        workspace.root(),
        &workspace.paths().user_transaction_lock(),
        changes,
    )?;
    Ok(moved)
}

/// Every raw assignment value recorded in the portable task files.
pub(super) fn assignment_values(workspace: &WorkspaceContext) -> Result<Vec<String>> {
    let mut values = Vec::new();
    for path in assignment_paths(workspace) {
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
        };
        let csv = parse_csv_bytes(&bytes).context("read task assignment CSV")?;
        for row in &csv.rows {
            if let Some(value) = row.get("assigned_to") {
                values.push(value.clone());
            }
        }
    }
    Ok(values)
}

fn assignment_paths(workspace: &WorkspaceContext) -> [PathBuf; 2] {
    [
        workspace.root().join("tasks/tasks.csv"),
        workspace.root().join("tasks/habits.csv"),
    ]
}

fn rewritten_csv(path: &Path, from: &str, to: &UserId) -> Result<Option<RewrittenCsv>> {
    let before = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let mut csv = parse_csv_bytes(&before).context("read task assignment CSV")?;
    if !csv.header.iter().any(|header| header == "assigned_to") {
        return Ok(None);
    }
    let mut moved = 0;
    for row in &mut csv.rows {
        if row.get("assigned_to").map(|value| value.trim()) == Some(from) {
            moved += 1;
            row.insert("assigned_to".to_owned(), to.to_string());
        }
    }
    if moved == 0 {
        return Ok(Some((before.clone(), before, 0)));
    }
    let after = serialize_csv(&csv).context("write canonical task assignment CSV")?;
    Ok(Some((before, after, moved)))
}

#[cfg(test)]
mod tests {
    use super::unmapped_assignments;
    use crate::users::{User, UserId, Users};

    fn users() -> Users {
        Users {
            schema_version: 1,
            users: vec![User {
                id: UserId::parse("pablo").unwrap(),
                name: "Pablo".to_owned(),
                phones: Vec::new(),
                emails: Vec::new(),
                response_email: None,
            }],
        }
    }

    #[test]
    fn only_distinct_nonempty_values_outside_the_registry_are_reported_in_order() {
        let values = ["me", "pablo", " me ", "", "Wife", "Wife"].map(str::to_owned);

        assert_eq!(unmapped_assignments(&values, &users()), ["me", "Wife"]);
    }
}
