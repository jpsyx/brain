use super::planning::{file_snapshot, patched_config, validate_config_patch};
use super::{
    ArtifactKind, Journal, ManagedSkillManifest, Result, ensure, instructions, snapshot, sources,
};
pub(in crate::skills::install_transaction) fn validate_metadata(
    manifest: &ManagedSkillManifest,
) -> Result<()> {
    ensure!(
        manifest.schema_version == 2 || manifest.package.is_none(),
        "schema-1 manifest has package metadata"
    );
    if let Some(owned) = manifest
        .package
        .as_ref()
        .and_then(|metadata| metadata.instructions.as_ref())
    {
        ensure!(
            owned.receipt.schema_version == 1
                && (1..=2).contains(&owned.receipt.generated_revision)
                && super::super::validation::hash(&owned.receipt.body_sha256)
                && super::super::validation::hash(&owned.receipt.sources_sha256),
            "invalid instruction ownership receipt"
        );
        if let Some(receipt) = &owned.original_receipt {
            instructions::verified_body(
                owned
                    .original
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("missing adopted instruction original"))?,
                receipt,
            )?;
        }
        let mut end = 0;
        for span in &owned.removed_legacy {
            ensure!(
                span.range.start >= end
                    && span.range.start < span.range.end
                    && owned
                        .original
                        .as_ref()
                        .and_then(|bytes| bytes.get(span.range.clone()))
                        == Some(span.bytes.as_slice()),
                "invalid retained instruction span"
            );
            end = span.range.end;
        }
    }
    if let Some(metadata) = &manifest.package {
        for patch in &metadata.config_history {
            validate_config_patch(patch)?;
        }
        for source in &metadata.source_history {
            sources::validate(source)?;
        }
    }
    Ok(())
}
pub(in crate::skills::install_transaction) fn validate_artifacts(
    journal: &Journal,
    next: &ManagedSkillManifest,
    old: Option<&ManagedSkillManifest>,
) -> Result<()> {
    ensure!(
        journal.schema_version == 2 || journal.artifacts.is_empty(),
        "schema-1 package artifacts"
    );
    for (path, change) in &journal.artifacts {
        match change.kind {
            ArtifactKind::InstructionRestore => {
                ensure!(
                    path == "AGENTS.md"
                        && change.config.is_none()
                        && change.source.is_none()
                        && journal.retained_package.is_some(),
                    "invalid instruction restoration"
                );
                let owned = old
                    .and_then(|manifest| manifest.package.as_ref())
                    .and_then(|metadata| metadata.instructions.as_ref())
                    .ok_or_else(|| {
                        anyhow::anyhow!("instruction restoration has no prior ownership")
                    })?;
                ensure!(
                    super::legacy::instruction_output(
                        change.before.as_deref(),
                        owned,
                        change.target_contract.as_ref(),
                        &next.binary_version
                    )? == change.after,
                    "unverified instruction restoration"
                );
            }
            ArtifactKind::Instructions => ensure!(
                path == "AGENTS.md"
                    && change.target_contract.is_none()
                    && change.config.is_none()
                    && change.source.is_none(),
                "invalid instruction artifact"
            ),
            ArtifactKind::AllowedSkills => {
                let patch = change
                    .config
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing config patch"))?;
                ensure!(
                    path == ".config/config.json"
                        && change.target_contract.is_none()
                        && change.source.is_none()
                        && Some(patched_config(change.before.as_deref(), patch)?) == change.after,
                    "invalid configuration replacement"
                );
            }
            ArtifactKind::LegacySource => {
                let source = change
                    .source
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing source provenance"))?;
                sources::validate(source)?;
                ensure!(
                    change.target_contract.is_none()
                        && source.patch.relative_path == std::path::Path::new(path)
                        && source.patch.before == change.before
                        && Some(&source.patch.after) == change.after.as_ref()
                        && change.config.is_none(),
                    "invalid source replacement"
                );
            }
        }
        let entry = journal
            .entries
            .iter()
            .find(|entry| &entry.live == path)
            .ok_or_else(|| anyhow::anyhow!("missing package entry"))?;
        ensure!(
            entry.kind == Some(change.kind)
                && entry.before == file_snapshot(change.before.as_deref(), &entry.before)
                && entry.after == file_snapshot(change.after.as_deref(), &entry.before),
            "package fingerprint mismatch"
        );
        if change.kind != ArtifactKind::Instructions {
            continue;
        }
        let receipt = &next
            .package
            .as_ref()
            .and_then(|metadata| metadata.instructions.as_ref())
            .ok_or_else(|| anyhow::anyhow!("missing instruction ownership"))?
            .receipt;
        let body =
            instructions::verified_body(change.after.as_deref().unwrap_or_default(), receipt)?;
        let prior = old
            .and_then(|manifest| manifest.package.as_ref())
            .and_then(|metadata| metadata.instructions.as_ref());
        let expected = instructions::plan_document(
            change.before.as_deref(),
            body,
            prior.map(|owned| &owned.receipt),
        )?;
        ensure!(
            Some(expected.after) == change.after,
            "invalid instruction replacement"
        );
    }
    if let Some(metadata) = &journal.retained_package {
        ensure!(
            journal.schema_version == 2 && next.schema_version == 1 && next.package.is_none(),
            "invalid legacy package export"
        );
        let mut retained = next.clone();
        retained.schema_version = 2;
        retained.package = Some(metadata.clone());
        validate_metadata(&retained)?;
        if old
            .and_then(|manifest| manifest.package.as_ref())
            .and_then(|metadata| metadata.instructions.as_ref())
            .is_some()
        {
            ensure!(
                journal
                    .artifacts
                    .get("AGENTS.md")
                    .is_some_and(|change| change.kind == ArtifactKind::InstructionRestore),
                "legacy export omitted owned instructions"
            );
        }
    }
    super::transitions::validate(journal, next, old)?;
    ensure!(
        journal
            .entries
            .iter()
            .filter(|entry| entry.kind.is_some())
            .count()
            == journal.artifacts.len(),
        "unrecorded package entry"
    );
    Ok(())
}

pub(in crate::skills::install_transaction) fn check_owned(
    identity: &super::InstallIdentity,
    manifest: &ManagedSkillManifest,
) -> Result<Vec<String>> {
    let Some(owned) = manifest
        .package
        .as_ref()
        .and_then(|metadata| metadata.instructions.as_ref())
    else {
        return Ok(Vec::new());
    };
    snapshot::check_parent(&identity.root, "AGENTS.md")?;
    let current = snapshot::read_optional_file(&identity.root.join("AGENTS.md"))?;
    let Some(current) = current else {
        return Ok(vec!["AGENTS.md".into()]);
    };
    if !instructions::has_managed_region(&current)? {
        return Ok(vec!["AGENTS.md".into()]);
    }
    instructions::verified_body(&current, &owned.receipt)?;
    Ok(Vec::new())
}
