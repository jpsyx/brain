//! The in-memory representation of a skill to install, shared by the embedded
//! bundled skills (`embed`) and the user's on-disk plugins (`plugin`).

use std::path::PathBuf;

/// A skill and every file under it (paths relative to the skill dir).
pub struct Skill {
    pub name: String,
    pub(crate) bundled: bool,
    pub files: Vec<SkillFile>,
}

pub struct SkillFile {
    pub rel_path: PathBuf,
    pub contents: Vec<u8>,
}

/// Checked compiler output keeps unselected projected data for package consumers.
pub(crate) struct CompiledSkills {
    pub skills: super::install_transaction::RenderedInstall,
    pub projection: super::compatibility::ExtensionProjection,
    pub workspace_body: String,
    pub sources_sha256: String,
}

impl CompiledSkills {
    pub(crate) fn compose_package(
        &self,
        existing: Option<&[u8]>,
        prior: Option<&crate::workspace::instructions::InstructionReceipt>,
    ) -> anyhow::Result<super::install_transaction::package::ManagedWorkspacePackage> {
        let mut document =
            crate::workspace::instructions::plan_document(existing, &self.workspace_body, prior)?;
        document
            .receipt
            .sources_sha256
            .clone_from(&self.sources_sha256);
        Ok(
            super::install_transaction::package::ManagedWorkspacePackage {
                skills: self.skills.clone(),
                extension_snapshot: Some(
                    super::install_transaction::package::ExtensionSnapshot::new(
                        self.projection.sources.clone(),
                    )?,
                ),
                instructions: Some(document),
                config_patches: Vec::new(),
                source_patches: self.projection.legacy_source_patches.clone(),
            },
        )
    }
}
