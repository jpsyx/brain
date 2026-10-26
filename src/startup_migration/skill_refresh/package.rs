//! Live read-only planning supplies projected replica inputs and one typed transaction.
use super::{
    replica,
    sources::Sources,
    target::{Profile, Target},
};
use crate::skills::{
    compatibility,
    install_transaction::{
        InstallVersion, LockedInstall, RenderedInstall, StampStore,
        package::{
            InstructionRestore, LegacyWorkspacePackage, ManagedWorkspacePackage,
            TargetInstructionContract, compose_target_instructions,
        },
    },
};
use anyhow::Result;

pub(super) enum Package {
    Current(ManagedWorkspacePackage),
    Legacy(LegacyWorkspacePackage),
}
impl Package {
    pub(super) fn prepare(
        locked: &LockedInstall,
        sources: &Sources,
        target: &Target,
    ) -> Result<Self> {
        if target.profile == Profile::Current {
            return Ok(Self::Current(
                crate::skills::reconcile::package::prepare_compiled(
                    locked,
                    &replica::compile(sources)?,
                    target.legacy_evidence,
                )?,
            ));
        }
        let manifest = locked.manifest()?;
        let metadata = manifest
            .as_ref()
            .and_then(|manifest| manifest.package.as_ref());
        let instructions = metadata
            .and_then(|metadata| metadata.instructions.as_ref())
            .map(|owned| {
                let before = sources.files.get("AGENTS.md").cloned();
                let contract = TargetInstructionContract {
                    binary_version: target.version.clone(),
                    bytes: include_bytes!("../../../templates/workspace/legacy/AGENTS-v0.105.3.md")
                        .to_vec(),
                };
                let after = compose_target_instructions(before.as_deref(), owned, &contract)?;
                Ok::<_, anyhow::Error>(InstructionRestore {
                    before,
                    after: Some(after),
                    target_contract: Some(contract),
                })
            })
            .transpose()?;
        let config = sources.files.get(".config/config.json").map_or_else(
            || Ok(serde_json::json!({})),
            |bytes| serde_json::from_slice(bytes),
        )?;
        let config_patches = if let Some(metadata) = metadata {
            compatibility::reverse_allowed_skills(&config, &metadata.config_history)?
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };
        let projection =
            compatibility::project_extensions(&sources.extensions()?, target.profile.direction())?;
        Ok(Self::Legacy(LegacyWorkspacePackage {
            skills: RenderedInstall::new(),
            instructions,
            config_patches,
            source_patches: projection.legacy_source_patches,
        }))
    }
    pub(super) fn projected_sources(&self, live: &Sources) -> Result<Sources> {
        let mut target = live.clone();
        let (instructions, config, sources) = match self {
            Self::Current(package) => (
                package.instructions.as_ref().map(|plan| Some(&plan.after)),
                &package.config_patches,
                &package.source_patches,
            ),
            Self::Legacy(package) => (
                package
                    .instructions
                    .as_ref()
                    .map(|plan| plan.after.as_ref()),
                &package.config_patches,
                &package.source_patches,
            ),
        };
        if let Some(instructions) = instructions {
            if let Some(bytes) = instructions {
                target.files.insert("AGENTS.md".into(), bytes.clone());
            } else {
                target.files.remove("AGENTS.md");
            }
        }
        for patch in sources {
            patch.validate_before(
                live.files
                    .get(&patch.relative_path.to_string_lossy().into_owned())
                    .map(Vec::as_slice),
            )?;
            target.files.insert(
                patch.relative_path.to_string_lossy().into_owned(),
                patch.after.clone(),
            );
        }
        for patch in config {
            let mut value: serde_json::Value =
                target.files.get(".config/config.json").map_or_else(
                    || Ok(serde_json::json!({})),
                    |bytes| serde_json::from_slice(bytes),
                )?;
            let object = value
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("invalid config object"))?;
            anyhow::ensure!(
                object.get(&patch.field) == patch.before.as_ref(),
                "config changed during target planning"
            );
            if let Some(after) = &patch.after {
                object.insert(patch.field.clone(), after.clone());
            } else {
                object.remove(&patch.field);
            }
            target.files.insert(
                ".config/config.json".into(),
                serde_json::to_vec_pretty(&value)?,
            );
        }
        Ok(target)
    }
    pub(super) fn set_skills(&mut self, output: RenderedInstall) {
        match self {
            Self::Current(package) => package.skills = output,
            Self::Legacy(package) => package.skills = output,
        }
    }
    pub(super) fn preflight(&self, locked: &LockedInstall, version: &InstallVersion) -> Result<()> {
        match self {
            Self::Current(package) => {
                locked.preflight_package(package, version)?;
            }
            Self::Legacy(package) => {
                locked.preflight_legacy_package(package, version)?;
            }
        }
        Ok(())
    }
    pub(super) fn install(
        self,
        locked: &mut LockedInstall,
        version: &InstallVersion,
        stamp: &mut dyn StampStore,
    ) -> Result<()> {
        match self {
            Self::Current(package) => {
                locked.install_package(package, version, stamp)?;
            }
            Self::Legacy(package) => {
                locked.install_legacy_package(package, version, stamp)?;
            }
        }
        Ok(())
    }
}
