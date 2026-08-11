//! Adopting an existing workspace's portable identity onto a new machine.
//!
//! The portable manifest is excluded from bisync on purpose, and every other
//! identity write publishes local → remote, so nothing ever brought it *down*.
//! A machine joining a synced workspace therefore had no way to obtain the
//! identity its first sync demands, and minting one locally is not a
//! substitute: `WorkspaceManifest::new` issues a fresh `receiver_ingress_id`,
//! so the joining machine would disagree with its peers about portable identity
//! forever, the manifest being the one file bisync never reconciles.

use std::path::Path;

use anyhow::{Context, Result, bail};

use super::{RemoteCommandOutput, read, remote_manifest_arg, run_remote_command};
use crate::sync::remote::Remote;
use crate::workspace::{WorkspaceId, WorkspaceManifest};

/// What resolving a missing local manifest against the remote concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestAdoption {
    /// The remote's manifest was written locally; portable identity is intact.
    Adopted,
    /// The remote carries no manifest, so the caller mints from the registry.
    RemoteHasNoManifest,
    /// A local manifest already exists and was left untouched.
    AlreadyLocal,
}

/// Write the remote's portable manifest locally when this machine has none.
pub fn adopt_remote_manifest(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &Remote,
) -> Result<ManifestAdoption> {
    adopt_remote_manifest_with(root, expected_id, remote, run_remote_command)
}

fn adopt_remote_manifest_with(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &Remote,
    mut run: impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<ManifestAdoption> {
    if WorkspaceManifest::path(root).exists() {
        return Ok(ManifestAdoption::AlreadyLocal);
    }
    let path = remote_manifest_arg(&remote.arg);
    let Some(bytes) = read::read_remote_file(&remote.env, &path, &mut run).bytes() else {
        return Ok(ManifestAdoption::RemoteHasNoManifest);
    };
    let manifest = WorkspaceManifest::parse(&bytes, env!("CARGO_PKG_VERSION"))
        .context("adopt the remote workspace manifest")?;
    if manifest.workspace_id() != expected_id {
        bail!(
            "remote workspace UUID {} does not match selected workspace UUID {expected_id}; \
             refusing to adopt another workspace's identity",
            manifest.workspace_id()
        );
    }
    let config = root.join(".config");
    std::fs::create_dir_all(&config)
        .with_context(|| format!("create the workspace config directory {}", config.display()))?;
    manifest
        .write_new(root)
        .context("write the adopted workspace manifest")?;
    Ok(ManifestAdoption::Adopted)
}

#[cfg(test)]
mod tests;
