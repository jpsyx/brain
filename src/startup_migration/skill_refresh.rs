//! Installer-only target rendering, ownership preflight and publication.
mod replica;
mod sources;
mod target;
use crate::{
    cli::{RefreshPhase, RefreshWorkspaceSkillsArgs},
    skills::{
        install_transaction::{InstallIdentity, InstallVersion, LockedInstall, RenderedInstall},
        receipt::WorkspaceStamp,
    },
    workspace::{MachineRegistry, RegistryStore, WorkspaceContext, WorkspaceName, WorkspaceRecord},
};
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};
use target::Target;

/// Refresh every available registered workspace with the actual target's output.
/// This route deliberately runs before ordinary startup or selected bootstrap.
pub fn refresh_workspace_skills(args: &RefreshWorkspaceSkillsArgs) -> Result<()> {
    let target = Target::validate(args)?;
    let home = super::home_dir()?;
    let store = RegistryStore::real();
    match fs::symlink_metadata(store.path()) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        other => {
            other.context("inspect workspace registry")?;
        }
    }
    let registry = RegistryStore::load_readable(store.path()).context("read workspace registry")?;
    let mut core = None;
    let mut failures = false;
    eprintln!(
        "{}",
        crate::theme::Theme::active()
            .info("Preparing private temporary copies of workspace skill sources")
    );
    for (name, record) in &registry.workspaces {
        match fs::metadata(&record.root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("{name}: pending_unavailable ({})", record.root.display());
                continue;
            }
            _ => {}
        }
        match refresh(
            &home, &store, &registry, name, record, &target, &mut core, args.phase,
        ) {
            Ok(()) => eprintln!(
                "{name}: {}",
                if args.phase == RefreshPhase::Apply {
                    "updated"
                } else {
                    "ready"
                }
            ),
            Err(error) => {
                failures = true;
                eprintln!("{name}: pending_failed: {error}");
            }
        }
    }
    ensure!(!failures, "workspace skill refresh has pending failures");
    Ok(())
}
#[allow(clippy::too_many_arguments)] // Explicit immutable registry/target bindings, no hidden process state.
fn refresh(
    home: &Path,
    store: &RegistryStore,
    registry: &MachineRegistry,
    name: &WorkspaceName,
    record: &WorkspaceRecord,
    target: &Target,
    core: &mut Option<RenderedInstall>,
    phase: RefreshPhase,
) -> Result<()> {
    let workspace = WorkspaceContext::new(
        home,
        record.workspace_id,
        name.clone(),
        &record.root,
        &record.local_user_id,
        home,
    )?;
    let identity = InstallIdentity::registered(
        &record.root,
        record.workspace_id,
        workspace.paths().cache_dir(),
    )
    .context("validate workspace identity")?;
    let sources = sources::Sources::read(registry, name, record, &target.version)
        .context("validate target workspace inputs")?;
    if target.legacy {
        validate_schema(&record.root).context("target task schema is incompatible")?;
    }
    identity.prepare_cache()?;
    let _operations = crate::tasks::removal::prepare_downgrade(&workspace)
        .context("settle local removal recovery before target refresh")?;
    if core.is_none() {
        *core = Some(
            replica::render(target, name, record, &sources, false)
                .context("render target core inventory")?,
        );
    }
    let output = replica::render(target, name, record, &sources, true)
        .context("render target workspace skills")?;
    replica::validate(core.as_ref().expect("core rendered"), &output, &sources)
        .context("validate complete target output")?;
    let mut stamp = WorkspaceStamp::selected(&workspace);
    let mut locked = LockedInstall::acquire(&identity)?;
    locked.recover(&mut stamp)?;
    let current =
        RegistryStore::load_readable(store.path()).context("revalidate workspace registry")?;
    ensure!(
        current == *registry,
        "workspace registry changed during refresh"
    );
    ensure!(
        sources::Sources::read(&current, name, record, &target.version)? == sources,
        "workspace skill sources changed during refresh"
    );
    target.verify()?;
    let version = InstallVersion {
        binary_version: target.version.clone(),
        compatibility_revision: u32::from(!target.legacy),
    };
    locked
        .preflight(&output, &version)
        .context("validate managed ownership")?;
    if phase == RefreshPhase::Apply {
        locked
            .install(output, &version, &mut stamp)
            .context("publish actual target skills")?;
    }
    Ok(())
}
fn validate_schema(root: &Path) -> Result<()> {
    if let Some(bytes) = sources::optional_file(&root.join("tasks/SCHEMA.json"))? {
        let schema: serde_json::Value = serde_json::from_slice(&bytes)?;
        ensure!(schema.is_object(), "invalid task schema");
        if let Some(version) = schema.get("task_schema_version") {
            ensure!(
                version.as_u64().is_some_and(|v| v <= 2),
                "unsupported task schema version"
            );
        }
    }
    Ok(())
}
