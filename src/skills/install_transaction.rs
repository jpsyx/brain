//! Recoverable publication of the whole managed skill installation.
mod capabilities;
mod disposal;
mod identity;
mod inspection;
mod journal;
mod legacy;
mod ownership;
pub(crate) mod package;
#[cfg(test)]
mod package_archive_tests;
#[cfg(test)]
mod package_conflict_tests;
#[cfg(test)]
mod package_legacy_tests;
#[cfg(test)]
mod package_patch_tests;
#[cfg(test)]
mod package_recovery_tests;
#[cfg(test)]
mod package_tests;
mod snapshot;
mod validation;
pub(crate) use identity::{InstallIdentity, SyncBusy};
pub(crate) use inspection::{Currentness, InstallBusy};

use anyhow::{Result, ensure};
use journal::{Journal, Phase};
use serde::{Deserialize, Serialize};
use snapshot::Snapshot;
use std::{collections::BTreeMap, fs};

pub(crate) type RenderedInstall = BTreeMap<String, BTreeMap<String, Vec<u8>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManagedSkillManifest {
    pub schema_version: u32,
    pub workspace_id: String,
    pub binary_version: String,
    pub compatibility_revision: u32,
    pub files: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<package::PackageMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallVersion {
    pub binary_version: String,
    pub compatibility_revision: u32,
}
impl InstallVersion {
    pub(crate) fn current() -> Self {
        Self {
            binary_version: super::current_version().into(),
            compatibility_revision: 2,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallReceipt {
    pub version: InstallVersion,
    pub manifest_sha256: String,
}

pub(crate) trait StampStore {
    fn write(&mut self, receipt: &InstallReceipt) -> Result<()>;
    fn read(&self) -> Result<Option<InstallReceipt>> {
        Ok(None)
    }
}
pub(crate) struct NoStamp;
impl StampStore for NoStamp {
    fn write(&mut self, _: &InstallReceipt) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Checkpoint {
    Staged,
    BackedUp(usize),
    Installed(usize),
    ManifestPublished,
}

#[derive(Debug)]
pub(crate) struct RecoveredInstall {
    pub receipt: InstallReceipt,
    pub report: super::install::Report,
}

pub(crate) struct LockedInstall {
    identity: InstallIdentity,
    _lock: fs::File,
}
impl LockedInstall {
    pub(crate) fn read_package_file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        ensure!(
            matches!(path, "AGENTS.md" | ".config/config.json"),
            "unsupported package input"
        );
        snapshot::check_parent(&self.identity.root, path)?;
        snapshot::read_optional_file(&self.identity.root.join(path))
    }
    pub(crate) fn preflight(
        &self,
        rendered: &RenderedInstall,
        version: &InstallVersion,
    ) -> Result<super::install::Report> {
        self.preflight_package(
            &package::ManagedWorkspacePackage {
                extension_snapshot: None,
                skills: rendered.clone(),
                instructions: None,
                config_patches: Vec::new(),
                source_patches: Vec::new(),
            },
            version,
        )
    }
    pub(crate) fn manifest(&self) -> Result<Option<ManagedSkillManifest>> {
        let parsed: Option<ManagedSkillManifest> =
            snapshot::read_optional_file(&self.identity.cache.join("managed.json"))?
                .as_deref()
                .map(serde_json::from_slice)
                .transpose()?;
        if let Some(manifest) = &parsed {
            validation::manifest(manifest, &self.identity)?;
        }
        Ok(parsed)
    }

    pub(crate) fn install(
        &mut self,
        rendered: RenderedInstall,
        version: &InstallVersion,
        stamp: &mut dyn StampStore,
    ) -> Result<InstallReceipt> {
        self.install_package(
            package::ManagedWorkspacePackage {
                extension_snapshot: None,
                skills: rendered,
                instructions: None,
                config_patches: Vec::new(),
                source_patches: Vec::new(),
            },
            version,
            stamp,
        )
    }

    #[cfg(test)]
    pub(super) fn install_with(
        &mut self,
        rendered: RenderedInstall,
        version: &InstallVersion,
        stamp: &mut dyn StampStore,
        checkpoint: &mut dyn FnMut(Checkpoint) -> Result<()>,
    ) -> Result<InstallReceipt> {
        self.install_package_inner(
            package::ManagedWorkspacePackage {
                extension_snapshot: None,
                skills: rendered,
                instructions: None,
                config_patches: Vec::new(),
                source_patches: Vec::new(),
            },
            version,
            stamp,
            checkpoint,
        )
    }

    fn publish(
        &mut self,
        plan: &ownership::Plan,
        previous: Option<Vec<u8>>,
        stamp: &mut dyn StampStore,
        checkpoint: &mut dyn FnMut(Checkpoint) -> Result<()>,
    ) -> Result<InstallReceipt> {
        let journal = Journal::planned(&self.identity, previous, plan)?;
        journal.retain_package(&self.identity.cache)?;
        let transaction_id = &journal.transaction_id;
        legacy::backup(&self.identity, transaction_id, &plan.legacy)?;
        for (index, entry) in plan.entries.iter().enumerate() {
            snapshot::ensure_parent(&self.identity.root, &entry.live)?;
            let stage = entry.slot(&self.identity.root, transaction_id, index, "stage");
            snapshot::materialize(
                &self.identity.root,
                &stage,
                &entry.live,
                &entry.after,
                &plan.files,
            )?;
            snapshot::sync_dir(stage.parent().expect("validated artifact parent"))?;
        }
        if let Err(error) = checkpoint(Checkpoint::Staged) {
            for (index, entry) in plan.entries.iter().enumerate() {
                snapshot::remove(
                    &entry.slot(&self.identity.root, transaction_id, index, "stage"),
                    &entry.after,
                )?;
            }
            return Err(error);
        }
        journal.save(&self.identity.cache)?;
        validation::journal(&journal, &self.identity)?;
        journal.verify_before(&self.identity.root)?;
        let result = self.apply(&journal, stamp, checkpoint);
        if result.is_err()
            && (snapshot::read_optional_file(&self.identity.cache.join("managed.json"))?.as_ref()
                != Some(&journal.next)
                || (journal.previous.as_ref() == Some(&journal.next)
                    && journal.verify_after(&self.identity.root).is_err()))
        {
            self.recover(stamp)?;
        }
        result
    }

    fn apply(
        &self,
        journal: &Journal,
        stamp: &mut dyn StampStore,
        checkpoint: &mut dyn FnMut(Checkpoint) -> Result<()>,
    ) -> Result<InstallReceipt> {
        for (index, entry) in journal.entries.iter().enumerate() {
            journal.verify_sources(&self.identity.root)?;
            snapshot::check_parent(&self.identity.root, &entry.live)?;
            let live = self.identity.root.join(&entry.live);
            ensure!(
                snapshot::read(&live)? == entry.before,
                "skills changed before replacement"
            );
            if entry.before != Snapshot::Absent {
                fs::rename(
                    &live,
                    entry.slot(
                        &self.identity.root,
                        &journal.transaction_id,
                        index,
                        "backup",
                    ),
                )?;
                snapshot::sync_dir(live.parent().expect("validated artifact parent"))?;
            }
            checkpoint(Checkpoint::BackedUp(index))?;
            if entry.after != Snapshot::Absent {
                fs::rename(
                    entry.slot(&self.identity.root, &journal.transaction_id, index, "stage"),
                    &live,
                )?;
            }
            snapshot::sync_dir(live.parent().expect("validated artifact parent"))?;
            checkpoint(Checkpoint::Installed(index))?;
        }
        journal.verify_after(&self.identity.root)?;
        snapshot::atomic_write(&self.identity.cache.join("managed.json"), &journal.next)?;
        let mut published = journal.clone();
        published.phase = Phase::Published;
        published.save(&self.identity.cache)?;
        checkpoint(Checkpoint::ManifestPublished)?;
        let receipt = journal.receipt()?;
        stamp
            .write(&receipt)
            .map_err(|_| super::diagnostic::Diagnostic::receipt_write())?;
        published.transition(Phase::DisposingCommitted, &self.identity.cache)?;
        published.cleanup(&self.identity.root, &self.identity.cache)?;
        Ok(receipt)
    }

    // Exclusive access prevents simultaneous filesystem transactions through one lock guard.
    #[allow(clippy::needless_pass_by_ref_mut)]
    pub(crate) fn recover(
        &mut self,
        stamp: &mut dyn StampStore,
    ) -> Result<Option<RecoveredInstall>> {
        let path = self.identity.cache.join("pending.json");
        let Some(bytes) = snapshot::read_optional_file(&path)? else {
            return Ok(None);
        };
        let mut journal: Journal = serde_json::from_slice(&bytes)?;
        validation::journal(&journal, &self.identity)?;
        journal.validate_slots(&self.identity.root)?;
        let actual = snapshot::read_optional_file(&self.identity.cache.join("managed.json"))?;
        let committed = match journal.phase {
            Phase::Published | Phase::DisposingCommitted => {
                ensure!(
                    actual.as_ref() == Some(&journal.next),
                    "published skills manifest changed during recovery"
                );
                true
            }
            Phase::RollingBack | Phase::DisposingRollback => {
                ensure!(
                    actual == journal.previous,
                    "skills rollback manifest conflict"
                );
                false
            }
            Phase::Prepared => {
                actual.as_ref() == Some(&journal.next)
                    && (journal.previous.as_ref() != Some(&journal.next)
                        || journal.verify_after(&self.identity.root).is_ok())
            }
        };
        let recovered = if committed {
            journal.verify_after(&self.identity.root)?;
            let receipt = journal.receipt()?;
            stamp
                .write(&receipt)
                .map_err(|_| super::diagnostic::Diagnostic::receipt_write())?;
            journal.transition(Phase::DisposingCommitted, &self.identity.cache)?;
            Some(RecoveredInstall {
                receipt,
                report: journal.report()?,
            })
        } else {
            ensure!(
                actual == journal.previous,
                "skills recovery manifest conflict"
            );
            if journal.phase == Phase::DisposingRollback {
                journal.verify_before(&self.identity.root)?;
            } else {
                journal.rollback(&self.identity.root, &self.identity.cache)?;
            }
            None
        };
        journal.cleanup(&self.identity.root, &self.identity.cache)?;
        Ok(recovered)
    }
}
