//! Compose one checked generation with its narrow compatibility field patch.
use super::super::{
    install,
    install_transaction::{LockedInstall, package::ManagedWorkspacePackage},
};
use anyhow::Result;

pub(super) fn prepare(
    locked: &LockedInstall,
    sources: &install::Sources,
    legacy_machine: bool,
) -> Result<ManagedWorkspacePackage> {
    let compiled = install::compile_sources(sources)?;
    prepare_compiled(locked, &compiled, legacy_machine)
}

pub(crate) fn prepare_compiled(
    locked: &LockedInstall,
    compiled: &crate::skills::model::CompiledSkills,
    legacy_machine: bool,
) -> Result<ManagedWorkspacePackage> {
    let existing = locked.read_package_file("AGENTS.md")?;
    let manifest = locked.manifest()?;
    let prior = manifest
        .as_ref()
        .and_then(|manifest| manifest.package.as_ref())
        .and_then(|package| package.instructions.as_ref())
        .map(|owned| &owned.receipt);
    let mut package = compiled.compose_package(existing.as_deref(), prior)?;
    let current_portable = existing.as_deref().is_some_and(|bytes| {
        crate::workspace::instructions::has_managed_region(bytes).unwrap_or(false)
            && (compiled
                .compose_package(Some(bytes), None)
                .is_ok_and(|package| {
                    package
                        .instructions
                        .is_some_and(|document| !document.changed)
                })
                || prior.is_some_and(|receipt| {
                    receipt.generated_revision >= 2
                        && crate::workspace::instructions::verified_body(bytes, receipt).is_ok()
                }))
    });
    let legacy_manifest = manifest
        .as_ref()
        .map(|manifest| {
            crate::startup_migration::brain_skills::predates_split(&manifest.binary_version)
        })
        .transpose()?
        .unwrap_or(false);
    let legacy_document = existing
        .as_deref()
        .is_some_and(crate::workspace::instructions::is_legacy_document);
    let mut retained_legacy = false;
    if let Some(manifest) = manifest
        .as_ref()
        .filter(|manifest| manifest.schema_version == 1)
    {
        let mut files = compiled.projection.sources.clone();
        for path in ["AGENTS.md", ".config/config.json"] {
            if let Some(bytes) = locked.read_package_file(path)? {
                files.insert(path.to_owned(), bytes);
            }
        }
        for candidate in locked.retained_packages()? {
            retained_legacy |= candidate.applies_to(manifest, &files)?;
        }
    }
    if !current_portable
        && (legacy_machine || legacy_manifest || legacy_document || retained_legacy)
    {
        let config = locked
            .read_package_file(".config/config.json")?
            .map_or_else(
                || Ok(serde_json::json!({})),
                |bytes| serde_json::from_slice(&bytes),
            )?;
        package
            .config_patches
            .extend(super::super::compatibility::project_allowed_skills(
                &config,
            )?);
    }
    Ok(package)
}
