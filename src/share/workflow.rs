use super::remote::{parse_link, signing_remote, validate_config};
use super::{create_archive, expiration, resolve_path};
use crate::sync::{config::SyncConfig, identity::VerifiedRemote, remote::Remote};
use crate::workspace::{CommandContext, WorkspaceContext};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Serialize, Deserialize)]
pub struct ShareResult {
    pub path: PathBuf,
    pub url: String,
    pub expires_at: String,
}

pub(super) trait Backend {
    fn verify<'a>(
        &mut self,
        workspace: &WorkspaceContext,
        remote: &'a Remote,
    ) -> Result<VerifiedRemote<'a>>;
    fn signing_endpoint(&mut self, config: &SyncConfig, duration: Duration) -> Result<String>;
    fn upload(&mut self, remote: VerifiedRemote<'_>, path: &Path, relative: &str) -> Result<()>;
    fn sign(&mut self, remote: &Remote, relative: &str, duration: Duration) -> Result<String>;
}

pub fn run(
    command: &CommandContext,
    input: &Path,
    expires: &str,
    progress: &mut dyn FnMut(&str),
) -> Result<ShareResult> {
    let duration = expiration(expires)?;
    let path = resolve_path(input, &std::env::current_dir()?, command.workspace.root())?;
    let config = SyncConfig::load(command);
    run_with(
        &command.workspace,
        &config,
        &path,
        duration,
        &mut super::transport::SystemBackend,
        progress,
    )
}

pub(super) fn run_with(
    workspace: &WorkspaceContext,
    config: &SyncConfig,
    path: &Path,
    duration: Duration,
    backend: &mut impl Backend,
    progress: &mut dyn FnMut(&str),
) -> Result<ShareResult> {
    let path = resolve_path(path, &std::env::current_dir()?, workspace.root())?;
    validate_config(config)?;
    if duration.as_secs() == 0 || duration.as_secs() > super::duration::MAX_SECONDS {
        bail!("Expiration must be between 1 second and 7 days");
    }
    progress("Checking the workspace sync lock");
    let _guard = crate::sync::lock::try_acquire(&workspace.paths().sync_lock())
        .context("A sync or share is already running, or the sync lock could not be acquired; try again after it finishes")?;
    crate::migration::require_no_active_rollout(workspace.paths())?;
    let native = crate::sync::remote::build_remote(config);
    progress("Verifying the selected workspace's remote identity");
    let verified = backend.verify(workspace, &native)?;
    progress("Checking private bucket access and download credentials");
    let endpoint = backend.signing_endpoint(config, duration)?;
    let signer = signing_remote(config, &endpoint)?;
    let shared = if path.is_dir() {
        progress("Creating the directory ZIP, excluding Brain metadata and tool artifacts");
        create_archive(&path)?
    } else {
        path
    };
    let root = workspace.root().canonicalize()?;
    let relative = shared
        .strip_prefix(&root)
        .context("Shared file is outside the selected workspace")?
        .to_str()
        .context("Shared path is not UTF-8")?;
    progress("Uploading the selected file to the private bucket");
    backend.upload(verified, &shared, relative)?;
    progress("Generating the temporary download link");
    let output = backend.sign(&signer, relative, duration)?;
    let link = parse_link(&output, duration.as_secs())?;
    Ok(ShareResult {
        path: shared,
        url: link.url,
        expires_at: link.expires_at.to_rfc3339(),
    })
}
