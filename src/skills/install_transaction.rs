//! Recoverable publication of the whole managed skill installation.
mod identity;
mod inspection;
mod journal;
mod legacy;
mod ownership;
mod snapshot;
mod validation;
pub(crate) use identity::InstallIdentity;
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
            compatibility_revision: 1,
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
    pub(crate) fn preflight(
        &self,
        rendered: &RenderedInstall,
        version: &InstallVersion,
    ) -> Result<super::install::Report> {
        ensure!(
            snapshot::read_optional_file(&self.identity.cache.join("pending.json"))?.is_none(),
            "skills recovery required before preflight"
        );
        let previous = self.manifest()?;
        let plan = ownership::plan(&self.identity, previous.as_ref(), rendered.clone(), version)?;
        let installed: Vec<_> = rendered.keys().chain(plan.users.keys()).cloned().collect();
        let old = previous
            .as_ref()
            .map_or_else(Vec::new, |m| ownership::roots(m).into_iter().collect());
        let pruned = super::prune::stale_rendered(&old, &installed);
        let retained = plan
            .legacy
            .keys()
            .filter(|name| !rendered.contains_key(*name))
            .cloned()
            .chain(
                plan.entries
                    .iter()
                    .filter(|entry| {
                        entry.live.starts_with(".agents/")
                            && entry.after != Snapshot::Absent
                            && !rendered.contains_key(
                                entry.live.split('/').nth(2).expect("validated skill root"),
                            )
                    })
                    .map(|entry| {
                        entry
                            .live
                            .split('/')
                            .nth(2)
                            .expect("validated skill root")
                            .to_owned()
                    }),
            )
            .collect();
        Ok(super::install::Report {
            installed,
            pruned,
            retained,
        })
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
        self.install_with(rendered, version, stamp, &mut |_| Ok(()))
    }

    pub(super) fn install_with(
        &mut self,
        rendered: RenderedInstall,
        version: &InstallVersion,
        stamp: &mut dyn StampStore,
        checkpoint: &mut dyn FnMut(Checkpoint) -> Result<()>,
    ) -> Result<InstallReceipt> {
        let _recovered = self.recover(stamp)?;
        let previous = snapshot::read_optional_file(&self.identity.cache.join("managed.json"))?;
        let previous_manifest: Option<ManagedSkillManifest> = previous
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()?;
        if let Some(previous) = &previous_manifest {
            validation::manifest(previous, &self.identity)?;
        }
        let plan = ownership::plan(
            &self.identity,
            previous_manifest.as_ref(),
            rendered,
            version,
        )?;
        let transaction_id = uuid::Uuid::new_v4().to_string();
        legacy::backup(&self.identity, &transaction_id, &plan.legacy)?;
        for (index, entry) in plan.entries.iter().enumerate() {
            snapshot::ensure_parent(&self.identity.root, &entry.live)?;
            let stage = entry.slot(&self.identity.root, &transaction_id, index, "stage");
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
                    &entry.slot(&self.identity.root, &transaction_id, index, "stage"),
                    &entry.after,
                )?;
            }
            return Err(error);
        }
        let next = serde_json::to_vec(&plan.manifest)?;
        let entries = plan.entries;
        let journal = Journal {
            users: plan.users,
            legacy: plan.legacy,
            phase: Phase::Prepared,
            previous_sha256: previous.as_ref().map(|bytes| snapshot::digest(bytes)),
            next_sha256: snapshot::digest(&next),
            schema_version: 1,
            workspace_id: self.identity.workspace_id.clone(),
            root: self.identity.root.clone(),
            transaction_id,
            previous,
            next,
            entries,
        };
        validation::journal(&journal, &self.identity)?;
        journal.save(&self.identity.cache)?;
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
        stamp.write(&receipt)?;
        journal.cleanup(&self.identity.root, &self.identity.cache)?;
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
        let journal: Journal = serde_json::from_slice(&bytes)?;
        validation::journal(&journal, &self.identity)?;
        journal.validate_slots(&self.identity.root)?;
        let actual = snapshot::read_optional_file(&self.identity.cache.join("managed.json"))?;
        ensure!(
            journal.phase != Phase::Published || actual.as_ref() == Some(&journal.next),
            "published skills manifest changed during recovery"
        );
        let recovered = if actual.as_ref() == Some(&journal.next)
            && (journal.previous.as_ref() != Some(&journal.next)
                || journal.phase == Phase::Published
                || journal.verify_after(&self.identity.root).is_ok())
        {
            journal.verify_after(&self.identity.root)?;
            let receipt = journal.receipt()?;
            stamp.write(&receipt)?;
            Some(RecoveredInstall {
                receipt,
                report: journal.report()?,
            })
        } else {
            ensure!(
                actual == journal.previous,
                "skills recovery manifest conflict"
            );
            journal.rollback(&self.identity.root)?;
            None
        };
        journal.cleanup(&self.identity.root, &self.identity.cache)?;
        Ok(recovered)
    }
}
