//! Atomic portable-user removal and task reassignment.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::users::{UserId, UserMutation, UsersStore, apply_mutation};
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
    let mut changes = vec![(users_path, users_before, users_after)];
    changes.extend(csv_changes);
    replace_group(changes)
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

fn replace_group(changes: Vec<(PathBuf, Vec<u8>, Vec<u8>)>) -> Result<()> {
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let mut staged = Vec::new();
    for (index, (path, before, after)) in changes.into_iter().enumerate() {
        let temporary = path.with_file_name(format!(".brain-user-{nonce}-{index}.tmp"));
        let backup = path.with_file_name(format!(".brain-user-{nonce}-{index}.backup"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&after)?;
        file.sync_all()?;
        staged.push((path, before, temporary, backup));
    }

    let result = (|| -> Result<()> {
        for (path, _, _, backup) in &staged {
            fs::rename(path, backup)?;
        }
        for (path, _, temporary, _) in &staged {
            fs::rename(temporary, path)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        for (path, before, temporary, backup) in &staged {
            let _ = fs::remove_file(temporary);
            if backup.exists() {
                let _ = fs::remove_file(path);
                if fs::rename(backup, path).is_err() {
                    let _ = fs::write(path, before);
                }
            }
        }
        return Err(error).context("atomically remove portable user and reassign tasks");
    }
    for (_, _, _, backup) in &staged {
        fs::remove_file(backup)?;
    }
    Ok(())
}
