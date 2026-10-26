//! Capture one reconciled instruction and extension generation before frontend work.
use super::{
    install_transaction::{InstallIdentity, LockedInstall, package::ExtensionSnapshot},
    receipt::WorkspaceStamp,
    reconcile::{RefreshMode, synchronize_locked},
};
use crate::workspace::WorkspaceContext;
use anyhow::{Context, Result, ensure};

#[derive(Clone)]
pub(crate) struct PreparedWorkspaceGeneration {
    pub instructions: Vec<u8>,
    pub extensions: ExtensionSnapshot,
}

pub(crate) fn prepare_launch(workspace: &WorkspaceContext) -> Result<PreparedWorkspaceGeneration> {
    let identity = InstallIdentity::workspace(workspace)?;
    let _sync = identity.acquire_sync()?;
    let mut locked = LockedInstall::acquire(&identity)?;
    synchronize_locked(
        &mut locked,
        &super::real_sources(workspace),
        &mut WorkspaceStamp::selected(workspace),
        RefreshMode::Compatibility,
        false,
    )?;
    let instructions = locked
        .read_package_file("AGENTS.md")?
        .context("missing published AGENTS.md")?;
    let metadata = locked
        .manifest()?
        .and_then(|manifest| manifest.package)
        .context("missing published workspace package")?;
    let extensions = metadata
        .extension_snapshot
        .context("published extension snapshot requires repair")?;
    let owned = metadata
        .instructions
        .context("missing published instruction ownership")?;
    let compiled = super::render::compile_catalog(&[], extensions.checked()?)?;
    ensure!(
        compiled.sources_sha256 == owned.receipt.sources_sha256
            && crate::workspace::instructions::verified_body(&instructions, &owned.receipt)?
                == compiled.workspace_body,
        "published instruction and extension generations differ"
    );
    Ok(PreparedWorkspaceGeneration {
        instructions,
        extensions,
    })
}

pub(crate) fn render_capabilities(
    generation: &PreparedWorkspaceGeneration,
    workspace: &WorkspaceContext,
    actor: &crate::actor::ActorContext,
    plan: &crate::access::CapabilityPlan,
) -> Result<super::WorkspaceCapabilityReport> {
    let identity = InstallIdentity::workspace(workspace)?;
    let _sync = identity.acquire_sync()?;
    let locked = LockedInstall::acquire(&identity)?;
    super::install::render_workspace_capabilities(
        &locked,
        actor.user_id(),
        &generation.extensions,
        plan,
    )
}
