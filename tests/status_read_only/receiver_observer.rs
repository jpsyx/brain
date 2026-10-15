//! Receiver reads may use SQLite coordination files, never mutate durable state.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::snapshot::{Entry, Referent};

pub(super) fn compare(
    before: &Entry,
    after: &Entry,
    workspace_ids: &[brain::workspace::WorkspaceId],
) -> Result<(), PathBuf> {
    let databases = workspace_ids
        .iter()
        .map(|id| PathBuf::from(format!(".cache/brain/workspaces/{id}/state.db")))
        .filter(|path| matches!(lookup(before, path), Some(Entry::File { .. })))
        .collect::<BTreeSet<_>>();
    compare_entry(Some(before), Some(after), Path::new(""), &databases)
}

fn lookup<'a>(entry: &'a Entry, path: &Path) -> Option<&'a Entry> {
    let mut entry = entry;
    for component in path.components() {
        if let Entry::Symlink { referent, .. } = entry {
            let Referent::Entry(referent) = referent.as_ref() else {
                return None;
            };
            entry = referent;
        }
        let Entry::Directory { children, .. } = entry else {
            return None;
        };
        entry = children.get(component.as_os_str())?;
    }
    Some(entry)
}

fn compare_entry(
    before: Option<&Entry>,
    after: Option<&Entry>,
    path: &Path,
    databases: &BTreeSet<PathBuf>,
) -> Result<(), PathBuf> {
    if before == after {
        return Ok(());
    }
    if databases.contains(&path.with_file_name("state.db")) {
        let unchanged_mode = match (before, after) {
            (None, Some(Entry::File { .. })) => true,
            (Some(Entry::File { mode: old, .. }), Some(Entry::File { mode: new, .. })) => old == new,
            _ => false,
        };
        if path.file_name().is_some_and(|name| name == "state.db-shm") && unchanged_mode {
            return Ok(());
        }
        if path.file_name().is_some_and(|name| name == "state.db-wal")
            && before.is_none()
            && matches!(after, Some(Entry::File { bytes, .. }) if bytes.is_empty())
        {
            return Ok(());
        }
    }
    match (before, after) {
        (
            Some(Entry::Directory { mode: old_mode, children: old }),
            Some(Entry::Directory { mode: new_mode, children: new }),
        ) if old_mode == new_mode => {
            for name in old.keys().chain(new.keys().filter(|name| !old.contains_key(*name))) {
                compare_entry(old.get(name), new.get(name), &path.join(name), databases)?;
            }
            Ok(())
        }
        (
            Some(Entry::Symlink { mode: old_mode, target: old_target, referent: old }),
            Some(Entry::Symlink { mode: new_mode, target: new_target, referent: new }),
        ) if old_mode == new_mode && old_target == new_target => match (old.as_ref(), new.as_ref()) {
            (Referent::Entry(old), Referent::Entry(new)) => {
                compare_entry(Some(old), Some(new), path, databases)
            }
            _ => Err(path.to_path_buf()),
        },
        _ => Err(path.to_path_buf()),
    }
}

#[cfg(test)]
#[path = "receiver_observer_tests.rs"]
mod tests;
