//! Workspace identity decisions, setup ownership election, and the shared rclone manifest gate.

use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::sync::remote::Remote;
use crate::workspace::{ManifestError, WorkspaceId, WorkspaceManifest};

mod claim;
mod remote_command;

use remote_command::run_remote_command;

const REMOTE_MANIFEST: &str = ".config/workspace.json";

/// The only safe outcomes of comparing local and remote workspace identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteIdentityDecision {
    /// An empty remote may receive this workspace's manifest during setup.
    Initialize,
    /// The remote already belongs to this workspace.
    Proceed,
    /// The target belongs to a different workspace and must not be touched.
    RefuseMismatch {
        local: WorkspaceId,
        remote: WorkspaceId,
    },
    /// A nonempty target has no ownership manifest and cannot be adopted implicitly.
    RefuseMissingManifest,
    /// The target manifest cannot be trusted by this Brain version.
    RefuseInvalidManifest { error: ManifestError },
    /// The target listing proves a manifest exists, but it could not be read.
    RefuseUnreadableManifest { message: String },
}

/// What setup learned from the configured remote before making any write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteIdentityObservation {
    /// The target listed successfully and contained no files.
    Empty,
    /// The target contains files but no portable ownership manifest.
    ManifestlessNonempty,
    /// The target carries a schema-compatible portable manifest.
    CompatibleManifest { workspace_id: WorkspaceId },
    /// The target manifest cannot be trusted by this Brain version.
    InvalidManifest { error: ManifestError },
    /// The target lists an ownership manifest that rclone could not read.
    UnreadableManifest { message: String },
}

/// Explicit authority supplied only by `brain sync setup` after showing the
/// observed manifestless nonempty target to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestlessRemoteAdoption {
    Refuse,
    Authorized(WorkspaceId),
}

/// Compare one selected workspace ID with an optional remote owner.
#[must_use]
pub fn check_remote_identity(
    local: WorkspaceId,
    remote: Option<WorkspaceId>,
) -> RemoteIdentityDecision {
    match remote {
        None => RemoteIdentityDecision::Initialize,
        Some(remote) if remote == local => RemoteIdentityDecision::Proceed,
        Some(remote) => RemoteIdentityDecision::RefuseMismatch { local, remote },
    }
}

/// Parse remote manifest bytes and make the complete fail-closed identity decision.
#[must_use]
pub fn check_remote_manifest_identity(
    local: WorkspaceId,
    remote_manifest: Option<&[u8]>,
    remote_is_empty: bool,
    brain_version: &str,
) -> RemoteIdentityDecision {
    let observed = observe_remote_manifest(remote_manifest, remote_is_empty, brain_version);
    decision_from_observation(local, &observed)
}

fn observe_remote_manifest(
    remote_manifest: Option<&[u8]>,
    remote_is_empty: bool,
    brain_version: &str,
) -> RemoteIdentityObservation {
    let Some(bytes) = remote_manifest else {
        return if remote_is_empty {
            RemoteIdentityObservation::Empty
        } else {
            RemoteIdentityObservation::ManifestlessNonempty
        };
    };
    match WorkspaceManifest::parse(bytes, brain_version) {
        Ok(manifest) => RemoteIdentityObservation::CompatibleManifest {
            workspace_id: manifest.workspace_id(),
        },
        Err(error) => RemoteIdentityObservation::InvalidManifest { error },
    }
}

fn decision_from_observation(
    local: WorkspaceId,
    observed: &RemoteIdentityObservation,
) -> RemoteIdentityDecision {
    match observed {
        RemoteIdentityObservation::Empty => RemoteIdentityDecision::Initialize,
        RemoteIdentityObservation::ManifestlessNonempty => {
            RemoteIdentityDecision::RefuseMissingManifest
        }
        RemoteIdentityObservation::CompatibleManifest { workspace_id } => {
            check_remote_identity(local, Some(*workspace_id))
        }
        RemoteIdentityObservation::InvalidManifest { error } => {
            RemoteIdentityDecision::RefuseInvalidManifest {
                error: error.clone(),
            }
        }
        RemoteIdentityObservation::UnreadableManifest { message } => {
            RemoteIdentityDecision::RefuseUnreadableManifest {
                message: message.clone(),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteCommandOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: String,
}

/// A configured remote whose manifest matched the selected workspace.
#[derive(Debug, Clone, Copy)]
pub struct VerifiedRemote<'a> {
    remote: &'a Remote,
}

impl VerifiedRemote<'_> {
    /// The identity-checked rclone remote.
    #[must_use]
    pub const fn remote(&self) -> &Remote {
        self.remote
    }
}

fn remote_manifest_arg(remote_root: &str) -> String {
    format!("{}/{REMOTE_MANIFEST}", remote_root.trim_end_matches('/'))
}

pub(crate) fn validate_local_manifest(
    root: &Path,
    expected_id: WorkspaceId,
) -> Result<WorkspaceManifest> {
    let manifest = WorkspaceManifest::load(root, env!("CARGO_PKG_VERSION"))
        .context("validate local workspace manifest")?;
    if manifest.workspace_id() != expected_id {
        bail!(
            "selected workspace UUID {expected_id} does not match local manifest UUID {}",
            manifest.workspace_id()
        );
    }
    Ok(manifest)
}

/// Require the configured remote to carry a compatible manifest for this workspace.
pub fn require_remote_identity<'remote>(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &'remote Remote,
) -> Result<VerifiedRemote<'remote>> {
    require_remote_identity_with(root, expected_id, remote, run_remote_command)
}

fn require_remote_identity_with<'remote>(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &'remote Remote,
    mut run: impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<VerifiedRemote<'remote>> {
    let manifest = validate_local_manifest(root, expected_id)?;
    let observed = probe_remote_identity_with(remote, &mut run)?;
    match decision_from_observation(manifest.workspace_id(), &observed) {
        RemoteIdentityDecision::Proceed => Ok(VerifiedRemote { remote }),
        RemoteIdentityDecision::Initialize => {
            bail!("remote workspace is not initialized; run `brain sync setup`")
        }
        refusal => refuse(refusal),
    }
}

#[cfg(test)]
pub(crate) const fn verified_remote_for_test(remote: &Remote) -> VerifiedRemote<'_> {
    VerifiedRemote { remote }
}

/// During setup, publish the existing local manifest only when the remote is empty.
pub fn ensure_remote_identity_for_setup<'remote>(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &'remote Remote,
) -> Result<VerifiedRemote<'remote>> {
    ensure_remote_identity_for_setup_with(
        root,
        expected_id,
        remote,
        |_| Ok(ManifestlessRemoteAdoption::Refuse),
        run_remote_command,
    )
}

/// Inspect setup's configured target before making any remote write.
///
/// The callback presents the exact observation and collects any manifestless
/// adoption authority. Initialization then publishes and verifies the existing
/// local manifest.
pub fn ensure_remote_identity_for_setup_with_authorization<'remote>(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &'remote Remote,
    authorize: impl FnOnce(&RemoteIdentityObservation) -> Result<ManifestlessRemoteAdoption>,
) -> Result<VerifiedRemote<'remote>> {
    ensure_remote_identity_for_setup_with(root, expected_id, remote, authorize, run_remote_command)
}

fn ensure_remote_identity_for_setup_with<'remote>(
    root: &Path,
    expected_id: WorkspaceId,
    remote: &'remote Remote,
    authorize: impl FnOnce(&RemoteIdentityObservation) -> Result<ManifestlessRemoteAdoption>,
    mut run: impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<VerifiedRemote<'remote>> {
    let manifest = validate_local_manifest(root, expected_id)?;
    let observed = probe_remote_identity_with(remote, &mut run)?;
    let adoption = authorize(&observed)?;
    match decision_from_observation(manifest.workspace_id(), &observed) {
        RemoteIdentityDecision::Proceed => return Ok(VerifiedRemote { remote }),
        RemoteIdentityDecision::Initialize => {}
        RemoteIdentityDecision::RefuseMissingManifest
            if adoption == ManifestlessRemoteAdoption::Authorized(manifest.workspace_id()) => {}
        RemoteIdentityDecision::RefuseMissingManifest
            if matches!(adoption, ManifestlessRemoteAdoption::Authorized(_)) =>
        {
            let ManifestlessRemoteAdoption::Authorized(authorized) = adoption else {
                unreachable!("guard requires authorized adoption")
            };
            bail!(
                "remote adoption authority UUID {authorized} does not match selected workspace UUID {}",
                manifest.workspace_id()
            );
        }
        refusal => return refuse(refusal),
    }

    let local_path = WorkspaceManifest::path(root);
    let claim::Election::Winner(winner) =
        claim::register_and_elect(&local_path, manifest.workspace_id(), remote, &mut run)?
    else {
        bail!(
            "remote workspace ownership claim staged for UUID {}; no canonical owner or credentials were changed; run `brain sync setup` again after any competing setup attempt has finished",
            manifest.workspace_id()
        );
    };
    if winner != manifest.workspace_id() {
        bail!(
            "remote workspace ownership claim was won by UUID {winner}; selected workspace UUID {} was not published",
            manifest.workspace_id()
        );
    }

    let established = probe_remote_identity_with(remote, &mut run)?;
    match decision_from_observation(manifest.workspace_id(), &established) {
        RemoteIdentityDecision::Proceed => return Ok(VerifiedRemote { remote }),
        RemoteIdentityDecision::Initialize => {}
        RemoteIdentityDecision::RefuseMissingManifest
            if adoption == ManifestlessRemoteAdoption::Authorized(manifest.workspace_id()) => {}
        refusal => return refuse(refusal),
    }

    let remote_path = remote_manifest_arg(&remote.arg);
    let publish_args = vec![
        "copyto".to_owned(),
        local_path.to_string_lossy().into_owned(),
        remote_path.clone(),
        "--immutable".to_owned(),
    ];
    let published = run(&remote.env, &publish_args);
    if !published.success {
        bail!(
            "could not publish remote workspace manifest: {}",
            published.stderr.trim()
        );
    }

    let readback_args = vec!["cat".to_owned(), remote_path];
    let readback = run(&remote.env, &readback_args);
    if !readback.success {
        bail!(
            "could not verify published remote workspace manifest: {}",
            readback.stderr.trim()
        );
    }
    match check_remote_manifest_identity(
        manifest.workspace_id(),
        Some(&readback.stdout),
        false,
        env!("CARGO_PKG_VERSION"),
    ) {
        RemoteIdentityDecision::Proceed => Ok(VerifiedRemote { remote }),
        decision => refuse_publication(decision),
    }
}

fn probe_remote_identity_with(
    remote: &Remote,
    run: &mut impl FnMut(&[(String, String)], &[String]) -> RemoteCommandOutput,
) -> Result<RemoteIdentityObservation> {
    let manifest_path = remote_manifest_arg(&remote.arg);
    let cat_args = vec!["cat".to_owned(), manifest_path];
    let manifest = run(&remote.env, &cat_args);
    if manifest.success {
        return Ok(observe_remote_manifest(
            Some(&manifest.stdout),
            false,
            env!("CARGO_PKG_VERSION"),
        ));
    }

    let list_args = vec![
        "lsf".to_owned(),
        remote.arg.clone(),
        "--recursive".to_owned(),
        "--files-only".to_owned(),
    ];
    let listing = run(&remote.env, &list_args);
    if !listing.success {
        bail!(
            "could not inspect remote workspace identity: {}",
            listing.stderr.trim()
        );
    }
    if listing_contains_manifest(&listing.stdout) {
        return Ok(RemoteIdentityObservation::UnreadableManifest {
            message: manifest.stderr.trim().to_owned(),
        });
    }
    let empty = listing
        .stdout
        .split(|byte| *byte == b'\n')
        .all(|line| line.iter().all(u8::is_ascii_whitespace) || claim::is_claim_path(line));
    Ok(observe_remote_manifest(
        None,
        empty,
        env!("CARGO_PKG_VERSION"),
    ))
}

fn listing_contains_manifest(listing: &[u8]) -> bool {
    listing.split(|byte| *byte == b'\n').any(|line| {
        let start = line
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .unwrap_or(line.len());
        let end = line
            .iter()
            .rposition(|byte| !byte.is_ascii_whitespace())
            .map_or(start, |index| index + 1);
        &line[start..end] == REMOTE_MANIFEST.as_bytes()
    })
}

fn refuse<T>(decision: RemoteIdentityDecision) -> Result<T> {
    match decision {
        RemoteIdentityDecision::RefuseMismatch { local, remote } => {
            bail!("remote workspace UUID {remote} does not match selected workspace UUID {local}")
        }
        RemoteIdentityDecision::RefuseMissingManifest => {
            bail!("remote target has data but no workspace manifest; implicit adoption is refused")
        }
        RemoteIdentityDecision::RefuseInvalidManifest { error } => {
            bail!("remote workspace manifest is invalid or incompatible: {error}")
        }
        RemoteIdentityDecision::RefuseUnreadableManifest { message } => {
            bail!("remote workspace manifest is present but unreadable: {message}")
        }
        RemoteIdentityDecision::Initialize | RemoteIdentityDecision::Proceed => {
            bail!("remote identity decision was used in the wrong sync phase")
        }
    }
}

fn refuse_publication<T>(decision: RemoteIdentityDecision) -> Result<T> {
    refuse(decision).context("published remote workspace manifest failed read-back verification")
}

#[cfg(test)]
mod tests;
