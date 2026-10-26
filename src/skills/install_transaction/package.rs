//! Typed additional artifacts sharing the managed installation's commit boundary.
use super::{
    InstallIdentity, InstallReceipt, InstallVersion, LockedInstall, ManagedSkillManifest,
    RenderedInstall, StampStore,
    journal::{Entry, Journal},
    ownership::{self, Plan},
    snapshot::{self, Snapshot},
};
use crate::{
    skills::compatibility::SourcePatch,
    workspace::instructions::{self, DocumentPlan, InstructionReceipt, RemovedLegacySpan},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone)]
pub(crate) struct ManagedWorkspacePackage {
    pub skills: RenderedInstall,
    pub extension_snapshot: Option<ExtensionSnapshot>,
    pub instructions: Option<DocumentPlan>,
    pub config_patches: Vec<JsonFieldPatch>,
    pub source_patches: Vec<SourcePatch>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonFieldPatch {
    pub relative_path: PathBuf,
    pub field: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension_snapshot: Option<ExtensionSnapshot>,
    pub instructions: Option<InstructionOwnership>,
    pub config_history: Vec<JsonFieldPatch>,
    pub source_history: Vec<SourceOwnership>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstructionOwnership {
    pub receipt: InstructionReceipt,
    pub original: Option<Vec<u8>>,
    pub original_receipt: Option<InstructionReceipt>,
    pub removed_legacy: Vec<RemovedLegacySpan>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ArtifactKind {
    Instructions,
    InstructionRestore,
    AllowedSkills,
    LegacySource,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileChange {
    pub kind: ArtifactKind,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
    pub config: Option<JsonFieldPatch>,
    pub source: Option<SourceOwnership>,
    pub target_contract: Option<TargetInstructionContract>,
}

impl LockedInstall {
    pub(crate) fn preflight_package(
        &self,
        package: &ManagedWorkspacePackage,
        version: &InstallVersion,
    ) -> Result<crate::skills::install::Report> {
        ensure!(
            snapshot::read_optional_file(&self.identity.cache.join("pending.json"))?.is_none(),
            "skills recovery required before preflight"
        );
        let previous = self.manifest()?;
        let plan = plan(&self.identity, previous.as_ref(), package.clone(), version)?;
        Journal::planned(
            &self.identity,
            snapshot::read_optional_file(&self.identity.cache.join("managed.json"))?,
            &plan,
        )?
        .report()
    }
    pub(crate) fn install_package(
        &mut self,
        package: ManagedWorkspacePackage,
        version: &InstallVersion,
        stamp: &mut dyn StampStore,
    ) -> Result<InstallReceipt> {
        self.install_package_inner(package, version, stamp, &mut |_| Ok(()))
    }
    #[cfg(test)]
    pub(in crate::skills) fn install_package_with(
        &mut self,
        package: ManagedWorkspacePackage,
        version: &InstallVersion,
        stamp: &mut dyn StampStore,
        checkpoint: &mut dyn FnMut(super::Checkpoint) -> Result<()>,
    ) -> Result<InstallReceipt> {
        self.install_package_inner(package, version, stamp, checkpoint)
    }
    pub(super) fn install_package_inner(
        &mut self,
        package: ManagedWorkspacePackage,
        version: &InstallVersion,
        stamp: &mut dyn StampStore,
        checkpoint: &mut dyn FnMut(super::Checkpoint) -> Result<()>,
    ) -> Result<InstallReceipt> {
        let _recovered = self.recover(stamp)?;
        let previous = self.manifest()?;
        let plan = plan(&self.identity, previous.as_ref(), package, version)?;
        let previous = snapshot::read_optional_file(&self.identity.cache.join("managed.json"))?;
        self.publish(&plan, previous, stamp, checkpoint)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceOwnership {
    pub patch: SourcePatch,
    pub inputs: BTreeMap<String, Vec<u8>>,
}

pub(crate) mod archive;
mod extensions;
pub(crate) use extensions::ExtensionSnapshot;
mod legacy;
mod planning;
mod sources;
mod transitions;
mod validation;
pub(crate) use legacy::{compose_target_instructions, restore_instructions};
use planning::plan;
pub(super) use validation::{check_owned, validate_artifacts, validate_metadata};

#[cfg_attr(not(test), allow(dead_code))] // Audited target input, integrated by the installer next.
#[derive(Clone)]
pub(crate) struct InstructionRestore {
    pub target_contract: Option<TargetInstructionContract>,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}
#[cfg_attr(not(test), allow(dead_code))] // Audited target input, integrated by the installer next.
#[derive(Clone)]
pub(crate) struct LegacyWorkspacePackage {
    pub skills: RenderedInstall,
    pub instructions: Option<InstructionRestore>,
    pub config_patches: Vec<JsonFieldPatch>,
    pub source_patches: Vec<SourcePatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetInstructionContract {
    pub binary_version: String,
    pub bytes: Vec<u8>,
}
