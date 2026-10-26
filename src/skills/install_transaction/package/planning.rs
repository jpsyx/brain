use super::{
    ArtifactKind, Entry, FileChange, InstallVersion, InstructionOwnership, JsonFieldPatch,
    ManagedSkillManifest, ManagedWorkspacePackage, PackageMetadata, Plan, Result, Snapshot,
    check_owned, ensure, instructions, ownership, snapshot, sources, validate_metadata,
};
pub(super) fn plan(
    identity: &super::InstallIdentity,
    previous: Option<&ManagedSkillManifest>,
    package: ManagedWorkspacePackage,
    version: &InstallVersion,
) -> Result<Plan> {
    if package.instructions.is_none() {
        if let Some(previous) = previous {
            check_owned(identity, previous)?;
        }
    }
    let mut plan = ownership::plan(identity, previous, package.skills, version)?;

    if let Some(document) = package.instructions {
        let prior = previous
            .and_then(|manifest| manifest.package.as_ref())
            .and_then(|metadata| metadata.instructions.as_ref());
        let body = instructions::verified_body(&document.after, &document.receipt)?;
        let verified = instructions::plan_document(
            document.before.as_deref(),
            body,
            prior.map(|owned| &owned.receipt),
        )?;
        ensure!(
            verified.after == document.after
                && verified.removed_legacy == document.removed_legacy
                && verified.changed == document.changed,
            "invalid instruction plan"
        );
        add_file(
            identity,
            &mut plan,
            "AGENTS.md",
            FileChange {
                target_contract: None,
                kind: ArtifactKind::Instructions,
                before: document.before.clone(),
                after: Some(document.after),
                config: None,
                source: None,
            },
        )?;
        let metadata = plan
            .manifest
            .package
            .get_or_insert_with(PackageMetadata::default);
        let original_receipt = if let Some(prior) = prior {
            prior.original_receipt.clone()
        } else if document
            .before
            .as_deref()
            .map(instructions::has_managed_region)
            .transpose()?
            .unwrap_or(false)
        {
            Some(document.receipt.clone())
        } else {
            None
        };
        metadata.instructions = Some(InstructionOwnership {
            original_receipt,
            receipt: document.receipt,
            original: prior.map_or(document.before, |owned| owned.original.clone()),
            removed_legacy: prior.map_or(document.removed_legacy, |owned| {
                owned.removed_legacy.clone()
            }),
        });
        plan.manifest.schema_version = 2;
    }
    for patch in package.config_patches {
        validate_config_patch(&patch)?;
        let path = ".config/config.json";
        snapshot::check_parent(&identity.root, path)?;
        let before = snapshot::read_optional_file(&identity.root.join(path))?;
        let after = patched_config(before.as_deref(), &patch)?;
        add_file(
            identity,
            &mut plan,
            path,
            FileChange {
                target_contract: None,
                kind: ArtifactKind::AllowedSkills,
                before,
                after: Some(after),
                config: Some(patch.clone()),
                source: None,
            },
        )?;
        plan.manifest
            .package
            .get_or_insert_with(PackageMetadata::default)
            .config_history
            .push(patch);
    }
    for patch in package.source_patches {
        let ownership = sources::observe(identity, patch)?;
        let path = ownership
            .patch
            .relative_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 source path"))?
            .to_owned();
        add_file(
            identity,
            &mut plan,
            &path,
            FileChange {
                target_contract: None,
                kind: ArtifactKind::LegacySource,
                before: ownership.patch.before.clone(),
                after: Some(ownership.patch.after.clone()),
                config: None,
                source: Some(ownership.clone()),
            },
        )?;
        plan.manifest
            .package
            .get_or_insert_with(PackageMetadata::default)
            .source_history
            .push(ownership);
    }
    if let Some(snapshot) = package.extension_snapshot {
        ensure!(
            plan.artifacts.contains_key("AGENTS.md"),
            "extension snapshot requires instruction publication"
        );
        plan.manifest
            .package
            .get_or_insert_with(PackageMetadata::default)
            .extension_snapshot = Some(snapshot);
    }
    if plan.manifest.package.is_some() {
        plan.manifest.schema_version = 2;
    }
    validate_metadata(&plan.manifest)?;
    Ok(plan)
}

pub(super) fn add_file(
    identity: &super::InstallIdentity,
    plan: &mut Plan,
    path: &str,
    change: FileChange,
) -> Result<()> {
    snapshot::check_parent(&identity.root, path)?;
    ensure!(
        snapshot::read_optional_file(&identity.root.join(path))? == change.before,
        "package file changed before planning: {path}"
    );
    let mut entry = Entry::new(path.into(), &identity.root)?;
    entry.kind = Some(change.kind);
    entry.after = file_snapshot(change.after.as_deref(), &entry.before);
    if let Some(bytes) = &change.after {
        plan.files.insert(path.into(), bytes.clone());
    }
    ensure!(
        plan.artifacts.insert(path.into(), change).is_none(),
        "duplicate package path"
    );
    plan.entries.push(entry);
    Ok(())
}
pub(super) fn file_snapshot(bytes: Option<&[u8]>, before: &Snapshot) -> Snapshot {
    bytes.map_or(Snapshot::Absent, |bytes| Snapshot::File {
        sha256: snapshot::digest(bytes),
        mode: match before {
            Snapshot::File { mode, .. } => *mode,
            _ => 0o100_644,
        },
    })
}

pub(super) fn validate_config_patch(patch: &JsonFieldPatch) -> Result<()> {
    ensure!(
        patch.relative_path == std::path::Path::new(".config/config.json")
            && patch.field == "allowed_skills",
        "unsupported configuration field/path"
    );
    for value in [patch.before.as_ref(), patch.after.as_ref()]
        .into_iter()
        .flatten()
    {
        ensure!(
            value.is_null()
                || value
                    .as_array()
                    .is_some_and(|values| values.iter().all(serde_json::Value::is_string)),
            "invalid allowed_skills value"
        );
    }
    Ok(())
}
pub(super) fn patched_config(before: Option<&[u8]>, patch: &JsonFieldPatch) -> Result<Vec<u8>> {
    validate_config_patch(patch)?;
    let mut value: serde_json::Value =
        before.map_or_else(|| Ok(serde_json::json!({})), serde_json::from_slice)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("configuration is not an object"))?;
    ensure!(
        object.get(&patch.field) == patch.before.as_ref(),
        "allowed_skills changed before publication"
    );
    if let Some(after) = &patch.after {
        object.insert(patch.field.clone(), after.clone());
    } else {
        object.remove(&patch.field);
    }
    Ok(serde_json::to_vec_pretty(&value)?)
}
