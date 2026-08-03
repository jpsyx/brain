//! Atomic portable-user removal and task reassignment.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::users::{FileChange, UserId, UserMutation, UsersStore, apply_mutation, replace_group};
use crate::workspace::WorkspaceContext;

type CsvChange = (Vec<u8>, Vec<u8>, usize);

pub(super) fn remove_user(
    workspace: &WorkspaceContext,
    removed: &UserId,
    replacement: Option<&UserId>,
) -> Result<()> {
    let mut users = UsersStore::load(workspace)?;
    if users.user(removed).is_none() {
        return Err(crate::users::UsersError::UnknownUser {
            user_id: removed.to_string(),
        }
        .into());
    }
    if let Some(replacement) = replacement
        && (replacement == removed || users.user(replacement).is_none())
    {
        return Err(crate::users::UsersError::UnknownUser {
            user_id: replacement.to_string(),
        }
        .into());
    }

    let mut csv_changes = Vec::new();
    let mut assigned = 0;
    for path in [
        workspace.root().join("tasks/tasks.csv"),
        workspace.root().join("tasks/habits.csv"),
    ] {
        if let Some(change) = reassigned_csv(&path, removed, replacement)? {
            assigned += change.2;
            csv_changes.push((path, change.0, change.1));
        }
    }
    if assigned > 0 && replacement.is_none() {
        anyhow::bail!("tasks remain assigned to {removed}; use --reassign-to <USER_ID>");
    }
    apply_mutation(&mut users, UserMutation::Remove(removed.clone()))?;
    let users_path = UsersStore::path(workspace);
    let users_before = fs::read(&users_path).context("read portable users before removal")?;
    let users_after = users.to_bytes()?;
    let mut changes = csv_changes
        .into_iter()
        .map(|(path, before, after)| FileChange::new(path, before, after))
        .collect::<Vec<_>>();
    changes.push(FileChange::new(users_path, users_before, users_after));
    replace_group(
        workspace.root(),
        &workspace.paths().user_transaction_lock(),
        changes,
    )
    .map_err(Into::into)
}

fn reassigned_csv(
    path: &Path,
    removed: &UserId,
    replacement: Option<&UserId>,
) -> Result<Option<CsvChange>> {
    let before = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(before.as_slice());
    let headers = reader
        .headers()
        .context("read task assignment headers")?
        .clone();
    let Some(index) = headers
        .iter()
        .position(|header| header == "assigned_to")
        .or_else(|| headers.iter().position(|header| header == "assignee"))
    else {
        return Ok(Some((before.clone(), before, 0)));
    };
    let mut records = Vec::new();
    let mut assigned = 0;
    for result in reader.records() {
        let mut record = result.context("read task assignment row")?;
        if record.get(index) == Some(removed.as_str()) {
            assigned += 1;
            if let Some(replacement) = replacement {
                record = record
                    .iter()
                    .enumerate()
                    .map(|(field_index, value)| {
                        if field_index == index {
                            replacement.as_str()
                        } else {
                            value
                        }
                    })
                    .collect();
            }
        }
        records.push(record);
    }
    if assigned == 0 || replacement.is_none() {
        return Ok(Some((before.clone(), before, assigned)));
    }
    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    writer.write_record(&headers)?;
    for record in records {
        writer.write_record(&record)?;
    }
    let after = writer
        .into_inner()
        .map_err(csv::IntoInnerError::into_error)?;
    Ok(Some((before, after, assigned)))
}
