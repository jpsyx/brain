//! Explicit restoration and schema-1 output share the regular package transaction.
use super::planning::add_file;
use super::{
    ArtifactKind, FileChange, InstallIdentity, InstallReceipt, InstallVersion,
    InstructionOwnership, InstructionReceipt, Journal, LegacyWorkspacePackage, LockedInstall,
    ManagedSkillManifest, ManagedWorkspacePackage, Plan, Result, StampStore,
    TargetInstructionContract, ensure, instructions, planning, snapshot,
};

#[cfg_attr(not(test), allow(dead_code))] // Audited target integration follows this transaction layer.
impl LockedInstall {
    pub(crate) fn preflight_legacy_package(
        &self,
        package: &LegacyWorkspacePackage,
        version: &InstallVersion,
    ) -> Result<crate::skills::install::Report> {
        ensure!(
            snapshot::read_optional_file(&self.identity.cache.join("pending.json"))?.is_none(),
            "skills recovery required before preflight"
        );
        let previous = self.manifest()?;
        let plan = plan_legacy(&self.identity, previous.as_ref(), package.clone(), version)?;
        Journal::planned(
            &self.identity,
            snapshot::read_optional_file(&self.identity.cache.join("managed.json"))?,
            &plan,
        )?
        .report()
    }
    pub(crate) fn install_legacy_package(
        &mut self,
        package: LegacyWorkspacePackage,
        version: &InstallVersion,
        stamp: &mut dyn StampStore,
    ) -> Result<InstallReceipt> {
        self.install_legacy_package_inner(package, version, stamp, &mut |_| Ok(()))
    }
    #[cfg(test)]
    pub(in crate::skills) fn install_legacy_package_with(
        &mut self,
        package: LegacyWorkspacePackage,
        version: &InstallVersion,
        stamp: &mut dyn StampStore,
        checkpoint: &mut dyn FnMut(super::super::Checkpoint) -> Result<()>,
    ) -> Result<InstallReceipt> {
        self.install_legacy_package_inner(package, version, stamp, checkpoint)
    }
    fn install_legacy_package_inner(
        &mut self,
        package: LegacyWorkspacePackage,
        version: &InstallVersion,
        stamp: &mut dyn StampStore,
        checkpoint: &mut dyn FnMut(super::super::Checkpoint) -> Result<()>,
    ) -> Result<InstallReceipt> {
        let _recovered = self.recover(stamp)?;
        let previous = self.manifest()?;
        let plan = plan_legacy(&self.identity, previous.as_ref(), package, version)?;
        let previous = snapshot::read_optional_file(&self.identity.cache.join("managed.json"))?;
        self.publish(&plan, previous, stamp, checkpoint)
    }
}
#[cfg_attr(not(test), allow(dead_code))]
fn plan_legacy(
    identity: &InstallIdentity,
    previous: Option<&ManagedSkillManifest>,
    package: LegacyWorkspacePackage,
    version: &InstallVersion,
) -> Result<Plan> {
    let prior = previous
        .and_then(|manifest| manifest.package.as_ref())
        .and_then(|metadata| metadata.instructions.as_ref());
    ensure!(
        prior.is_some() == package.instructions.is_some(),
        "legacy package requires explicit restoration for owned instructions"
    );
    let mut plan = planning::plan(
        identity,
        previous,
        ManagedWorkspacePackage {
            skills: package.skills,
            instructions: None,
            config_patches: package.config_patches,
            source_patches: package.source_patches,
        },
        version,
    )?;
    if let (Some(owned), Some(restore)) = (prior, package.instructions) {
        ensure!(
            instruction_output(
                restore.before.as_deref(),
                owned,
                restore.target_contract.as_ref(),
                &version.binary_version
            )? == restore.after,
            "instruction restoration differs from retained ownership"
        );
        add_file(
            identity,
            &mut plan,
            "AGENTS.md",
            FileChange {
                target_contract: restore.target_contract,
                kind: ArtifactKind::InstructionRestore,
                before: restore.before,
                after: restore.after,
                config: None,
                source: None,
            },
        )?;
    }
    plan.retained_package = Some(plan.manifest.package.take().unwrap_or_default());
    plan.manifest.schema_version = 1;
    Ok(plan)
}

/// Remove only the verified managed frame; restore recognized legacy sections only
/// when the remaining original text still matches its retained adoption evidence.
pub(crate) fn restore_instructions(
    current: Option<&[u8]>,
    owned: &InstructionOwnership,
) -> Result<Option<Vec<u8>>> {
    let original = owned.original_receipt.as_ref().map_or_else(
        || Ok(owned.original.clone()),
        |receipt| {
            strip_frame(
                owned
                    .original
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("missing adopted instruction original"))?,
                receipt,
            )
            .map(Some)
        },
    )?;
    let Some(current) = current else {
        return Ok(original);
    };
    let body = instructions::verified_body(current, &owned.receipt)?;
    let unowned = strip_frame(current, &owned.receipt)?;
    let initial = instructions::plan_document(original.as_deref(), body, None)?;
    let original_unowned = strip_frame(&initial.after, &initial.receipt)?;
    if unowned == original_unowned {
        return Ok(original);
    }
    ensure!(
        owned.removed_legacy.is_empty(),
        "unowned instructions changed around removed legacy sections; restoration is ambiguous"
    );
    Ok(Some(unowned))
}
fn strip_frame(document: &[u8], receipt: &InstructionReceipt) -> Result<Vec<u8>> {
    let body = instructions::verified_body(document, receipt)?;
    let frame = instructions::plan_document(None, body, None)?.after;
    let offset = document
        .windows(frame.len())
        .position(|bytes| bytes == frame)
        .ok_or_else(|| anyhow::anyhow!("managed instruction frame missing"))?;
    let mut unowned = document.to_vec();
    unowned.drain(offset..offset + frame.len());
    Ok(unowned)
}

pub(super) fn instruction_output(
    current: Option<&[u8]>,
    owned: &InstructionOwnership,
    contract: Option<&TargetInstructionContract>,
    target_version: &str,
) -> Result<Option<Vec<u8>>> {
    if let Some(contract) = contract {
        ensure!(
            contract.binary_version == target_version,
            "instruction contract belongs to a different target binary"
        );
        super::compose_target_instructions(current, owned, contract).map(Some)
    } else {
        super::restore_instructions(current, owned)
    }
}

/// The caller supplies the audited target's core text as desired output. Ownership
/// of existing bytes still comes exclusively from the verified managed frame.
pub(crate) fn compose_target_instructions(
    current: Option<&[u8]>,
    owned: &InstructionOwnership,
    contract: &TargetInstructionContract,
) -> Result<Vec<u8>> {
    ensure!(
        super::super::validation::version(&contract.binary_version)
            && !contract.bytes.is_empty()
            && !instructions::has_managed_region(&contract.bytes)?,
        "invalid legacy target instruction contract"
    );
    let mut result = current.unwrap_or_default().to_vec();
    if instructions::has_managed_region(&result)? {
        let body = instructions::verified_body(&result, &owned.receipt)?;
        let frame = instructions::plan_document(None, body, None)?.after;
        let offset = result
            .windows(frame.len())
            .position(|bytes| bytes == frame)
            .ok_or_else(|| anyhow::anyhow!("owned instruction frame disappeared"))?;
        result.splice(offset..offset + frame.len(), contract.bytes.iter().copied());
    } else {
        if !result.is_empty() && !result.ends_with(b"\n") {
            result.push(b'\n');
        }
        result.extend_from_slice(&contract.bytes);
    }
    Ok(result)
}
