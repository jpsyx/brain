//! Bind retained metadata to the exact authenticated previous generation and changes.
use super::{ArtifactKind, InstructionOwnership, Journal, ManagedSkillManifest, Result, ensure};

pub(super) fn validate(
    journal: &Journal,
    next: &ManagedSkillManifest,
    old: Option<&ManagedSkillManifest>,
) -> Result<()> {
    let previous = old.and_then(|manifest| manifest.package.as_ref());
    let metadata = journal.retained_package.as_ref().or(next.package.as_ref());
    let Some(metadata) = metadata else {
        ensure!(
            previous.is_none() && journal.artifacts.is_empty(),
            "package ownership was discarded"
        );
        return Ok(());
    };
    let prior = previous.cloned().unwrap_or_default();
    let instructions = if let Some(change) = journal
        .artifacts
        .get("AGENTS.md")
        .filter(|change| change.kind == ArtifactKind::Instructions)
    {
        let owned = metadata
            .instructions
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing instruction metadata"))?;
        let body = crate::workspace::instructions::verified_body(
            change.after.as_deref().unwrap_or_default(),
            &owned.receipt,
        )?;
        let plan = crate::workspace::instructions::plan_for_revision(
            change.before.as_deref(),
            body,
            prior.instructions.as_ref().map(|value| &value.receipt),
            owned.receipt.generated_revision,
        )?;
        let original_receipt = prior.instructions.as_ref().map_or_else(
            || {
                change
                    .before
                    .as_deref()
                    .is_some_and(|bytes| {
                        crate::workspace::instructions::verified_body(bytes, &owned.receipt).is_ok()
                    })
                    .then(|| owned.receipt.clone())
            },
            |prior| prior.original_receipt.clone(),
        );
        Some(InstructionOwnership {
            receipt: owned.receipt.clone(),
            original_receipt,
            original: prior
                .instructions
                .as_ref()
                .map_or_else(|| change.before.clone(), |value| value.original.clone()),
            removed_legacy: prior
                .instructions
                .as_ref()
                .map_or(plan.removed_legacy, |value| value.removed_legacy.clone()),
        })
    } else {
        prior.instructions.clone()
    };
    ensure!(
        metadata.instructions == instructions,
        "instruction retention differs from the previous generation"
    );
    if !journal
        .artifacts
        .get("AGENTS.md")
        .is_some_and(|change| change.kind == ArtifactKind::Instructions)
    {
        ensure!(
            metadata.extension_snapshot == prior.extension_snapshot,
            "extension snapshot changed without instruction publication"
        );
    }
    let configs: Vec<_> = journal
        .artifacts
        .values()
        .filter_map(|change| change.config.clone())
        .collect();
    ensure!(
        metadata.config_history.starts_with(&prior.config_history)
            && metadata.config_history[prior.config_history.len()..] == configs,
        "configuration retention differs from the journal"
    );
    let sources: Vec<_> = journal
        .artifacts
        .values()
        .filter_map(|change| change.source.as_ref())
        .collect();
    ensure!(
        metadata.source_history.starts_with(&prior.source_history)
            && metadata.source_history.len() == prior.source_history.len() + sources.len(),
        "source retention history changed"
    );
    let mut actual: Vec<_> = metadata.source_history[prior.source_history.len()..]
        .iter()
        .collect();
    actual.sort_by(|a, b| a.patch.relative_path.cmp(&b.patch.relative_path));
    ensure!(
        actual == sources,
        "source retention differs from the journal"
    );
    Ok(())
}
