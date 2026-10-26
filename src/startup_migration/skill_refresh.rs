//! Installer-only target rendering, ownership preflight and publication.
mod package;
mod replacement;
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
use target::{Profile, Target};

/// Refresh every available registered workspace with the actual target's output.
/// This route deliberately runs before ordinary startup or selected bootstrap.
pub fn refresh_workspace_skills(args: &RefreshWorkspaceSkillsArgs) -> Result<()> {
    let installation = super::invocation::Guard::exclusive()?;
    let target = Target::validate(args)?;
    let replacement = replacement::Replacement::prepare(args, &target)?;
    if replacement.is_some() {
        super::run_explicit(
            args.from_version.as_deref().unwrap_or(&target.version),
            &target.version,
        )?;
    }
    refresh_target(args, &target)?;
    if let Some(replacement) = replacement {
        replacement.publish(&target, installation)?;
    }
    Ok(())
}

fn refresh_target(args: &RefreshWorkspaceSkillsArgs, target: &Target) -> Result<()> {
    let home = super::home_dir()?;
    let store = RegistryStore::real();
    match fs::symlink_metadata(store.path()) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        other => {
            other.context("inspect workspace registry")?;
        }
    }
    let registry = RegistryStore::load_readable(store.path()).context("read workspace registry")?;
    if target.profile != Profile::Current {
        for (name, record) in &registry.workspaces {
            ensure!(
                fs::metadata(&record.root).is_ok(),
                "{name}: pending_unavailable; every registered root must be available before downgrade"
            );
        }
    }
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
            &home, &store, &registry, name, record, target, &mut core, args.phase,
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
                let diagnostic = crate::skills::diagnostic::Diagnostic::from_error(
                    &error,
                    "target_refresh",
                    None,
                );
                eprintln!(
                    "{name}: pending_failed ({}): {diagnostic}",
                    record.workspace_id
                );
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
    identity.prepare_cache()?;
    let _sync = identity.acquire_sync()?;
    ensure!(
        identity.receive_phase()?.is_none(),
        crate::skills::diagnostic::Diagnostic::new(
            crate::skills::diagnostic::Category::ReceivePending,
            "target_refresh",
            None,
        )
    );
    let sources = sources::Sources::read(registry, name, record, &target.version)
        .context("validate target workspace inputs")?;
    validate_schema(
        sources.files.get("tasks/SCHEMA.json"),
        target.profile.task_schema(),
    )
    .context("target task schema is incompatible")?;
    let _operations = crate::tasks::removal::prepare_downgrade(&workspace)
        .context("settle local removal recovery before target refresh")?;
    let mut stamp = WorkspaceStamp::selected(&workspace);
    let mut locked = LockedInstall::acquire(&identity)?;
    locked.recover(&mut stamp)?;
    let machine = crate::access::MachineCapabilityEnvironment::for_skill_ownership(
        record.workspace_id,
        &record.env,
    )?;
    crate::access::preflight_bundled_skill_ownership(&machine)?;
    let provenance = locked.retained_packages()?;
    let package = package::Package::prepare(&locked, &sources, target)?;
    let projected = package.projected_sources(&sources)?;
    if core.is_none() {
        *core = Some(
            replica::render(target, name, record, &sources, false)
                .context("render target core inventory")?,
        );
    }
    let package = replica::render_package(
        target,
        name,
        record,
        &projected,
        core.as_ref().expect("core rendered"),
        package,
    )
    .context("validate complete target package")?;
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
    ensure!(
        locked.retained_packages()? == provenance,
        "package provenance changed during target rendering"
    );
    target.verify()?;
    let version = InstallVersion {
        binary_version: target.version.clone(),
        compatibility_revision: target.profile.revision(),
    };
    package
        .preflight(&locked, &version)
        .context("validate managed ownership")?;
    if phase == RefreshPhase::Apply {
        package
            .install(&mut locked, &version, &mut stamp)
            .context("publish actual target package")?;
        ensure!(
            locked
                .manifest()?
                .is_some_and(|manifest| manifest.schema_version == target.profile.schema()),
            "target package schema mismatch"
        );
    }
    Ok(())
}
fn validate_schema(bytes: Option<&Vec<u8>>, maximum: u64) -> Result<()> {
    if let Some(bytes) = bytes {
        let schema: serde_json::Value = serde_json::from_slice(bytes)?;
        ensure!(schema.is_object(), "invalid task schema");
        if let Some(version) = schema.get("task_schema_version") {
            ensure!(
                version.as_u64().is_some_and(|v| v <= maximum),
                "unsupported task schema version"
            );
        }
    }
    Ok(())
}
