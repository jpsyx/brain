//! Give every registered workspace the user-managed `capture/` in-basket.
//!
//! Ordinary command bootstrap already self-heals the *selected* workspace, but
//! a machine can hold several and the user reaches them one at a time. An
//! upgrade provisions all of them at once, so the directory is there the first
//! time they look for it rather than the first time they run a command against
//! that particular workspace.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::workspace::{CAPTURE_DIRECTORY, ensure_capture_directory};

pub(super) fn up(_home: &Path) -> Result<()> {
    for root in workspace_roots() {
        ensure_capture_directory(&root)?;
    }
    Ok(())
}

/// Remove the in-basket a rollback leaves behind — but only when it is empty.
///
/// An older binary simply ignores a directory it doesn't know about, which is
/// a far better outcome than deleting the user's captured material to satisfy
/// a version transition. Emptiness is the whole guard: anything inside,
/// including a single file, keeps the directory.
pub(super) fn down(_home: &Path) -> Result<()> {
    for root in workspace_roots() {
        let capture = root.join(CAPTURE_DIRECTORY);
        if is_empty_directory(&capture)? {
            std::fs::remove_dir(&capture).with_context(|| {
                format!("remove the empty capture in-basket {}", capture.display())
            })?;
        }
    }
    Ok(())
}

/// Whether `path` is a directory with nothing in it. A path that is absent, or
/// is not a directory at all, is not something this migration removes.
fn is_empty_directory(path: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }
    let mut entries = std::fs::read_dir(path)
        .with_context(|| format!("inspect the capture in-basket {}", path.display()))?;
    Ok(entries.next().transpose()?.is_none())
}

/// Every registered workspace root that exists on this machine.
///
/// Automatic migrations run before legacy registry bootstrap, so an
/// unreadable or absent registry yields nothing rather than failing the
/// invocation that triggered the migration. A root that is missing (an
/// unmounted volume, a workspace registered from another machine) is skipped:
/// creating a directory there would manufacture the very empty workspace that
/// root resolution refuses to invent.
fn workspace_roots() -> Vec<PathBuf> {
    let store = crate::workspace::RegistryStore::real();
    if !store.path().exists() {
        return Vec::new();
    }
    let Ok(registry) = crate::workspace::RegistryStore::load_readable(store.path()) else {
        return Vec::new();
    };
    registry
        .workspaces
        .values()
        .map(|record| record.root.clone())
        .filter(|root| root.is_dir())
        .collect()
}
