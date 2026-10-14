//! Durable whole-install intent and authenticated artifact restoration.
use super::{
    InstallReceipt, InstallVersion, ManagedSkillManifest,
    snapshot::{self, Snapshot},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub live: String,
    pub before: Snapshot,
    pub after: Snapshot,
}
impl Entry {
    pub fn new(live: String, root: &Path) -> Result<Self> {
        Ok(Self {
            before: snapshot::read(&root.join(&live))?,
            live,
            after: Snapshot::Absent,
        })
    }
    pub fn slot(&self, root: &Path, transaction_id: &str, index: usize, role: &str) -> PathBuf {
        root.join(&self.live)
            .parent()
            .expect("validated artifact parent")
            .join(format!(".brain-skills-{transaction_id}-{index}-{role}"))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    pub phase: Phase,
    pub users: std::collections::BTreeMap<String, Snapshot>,
    pub legacy: std::collections::BTreeMap<String, String>,
    pub previous_sha256: Option<String>,
    pub next_sha256: String,
    pub schema_version: u32,
    pub workspace_id: String,
    pub root: PathBuf,
    pub transaction_id: String,
    pub previous: Option<Vec<u8>>,
    pub next: Vec<u8>,
    pub entries: Vec<Entry>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Phase {
    Prepared,
    Published,
}

impl Journal {
    pub fn save(&self, cache: &Path) -> Result<()> {
        snapshot::atomic_write(&cache.join("pending.json"), &serde_json::to_vec(self)?)
    }
    pub fn report(&self) -> Result<super::super::install::Report> {
        let manifest: ManagedSkillManifest = serde_json::from_slice(&self.next)?;
        let previous: Option<ManagedSkillManifest> = self
            .previous
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()?;
        let roots = super::ownership::roots(&manifest);
        let installed: Vec<_> = roots.iter().chain(self.users.keys()).cloned().collect();
        let old: Vec<_> = previous.as_ref().map_or_else(Vec::new, |manifest| {
            super::ownership::roots(manifest).into_iter().collect()
        });
        let pruned = super::super::prune::stale_rendered(&old, &installed);
        let retained = self
            .legacy
            .keys()
            .filter(|name| !roots.contains(*name))
            .cloned()
            .chain(
                self.entries
                    .iter()
                    .filter(|entry| {
                        entry.live.starts_with(".agents/")
                            && entry.after != Snapshot::Absent
                            && !roots.contains(
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
        Ok(super::super::install::Report {
            installed,
            pruned,
            retained,
        })
    }
    pub fn receipt(&self) -> Result<InstallReceipt> {
        let manifest: ManagedSkillManifest = serde_json::from_slice(&self.next)?;
        Ok(InstallReceipt {
            version: InstallVersion {
                binary_version: manifest.binary_version,
                compatibility_revision: manifest.compatibility_revision,
            },
            manifest_sha256: snapshot::digest(&self.next),
        })
    }
    pub fn verify_after(&self, root: &Path) -> Result<()> {
        for entry in &self.entries {
            snapshot::check_parent(root, &entry.live)?;
            ensure!(
                snapshot::read(&root.join(&entry.live))? == entry.after,
                "skills recovery conflict at {}",
                entry.live
            );
        }
        Ok(())
    }
    pub fn validate_rollback(&self, root: &Path) -> Result<()> {
        self.validate_slots(root)?;
        for (index, entry) in self.entries.iter().enumerate() {
            snapshot::check_parent(root, &entry.live)?;
            let observed = snapshot::read(&root.join(&entry.live))?;
            let backup = snapshot::read(&entry.slot(root, &self.transaction_id, index, "backup"))?;
            if backup != Snapshot::Absent || entry.before == Snapshot::Absent {
                ensure!(
                    observed == entry.after || observed == Snapshot::Absent,
                    "skills recovery conflict at {}",
                    entry.live
                );
            } else {
                ensure!(
                    observed == entry.before,
                    "skills original missing during recovery: {}",
                    entry.live
                );
            }
        }
        Ok(())
    }
    pub fn validate_slots(&self, root: &Path) -> Result<()> {
        for (index, entry) in self.entries.iter().enumerate() {
            snapshot::check_parent(root, &entry.live)?;
            for (role, expected) in [("stage", &entry.after), ("backup", &entry.before)] {
                let observed =
                    snapshot::read(&entry.slot(root, &self.transaction_id, index, role))?;
                ensure!(
                    observed == Snapshot::Absent || observed == *expected,
                    "skills recovery slot conflict"
                );
            }
        }
        Ok(())
    }
    pub fn rollback(&self, root: &Path) -> Result<()> {
        self.validate_rollback(root)?;
        for (index, entry) in self.entries.iter().enumerate().rev() {
            snapshot::check_parent(root, &entry.live)?;
            let live = root.join(&entry.live);
            let backup = entry.slot(root, &self.transaction_id, index, "backup");
            let observed = snapshot::read(&live)?;
            if snapshot::read(&backup)? != Snapshot::Absent {
                ensure!(
                    snapshot::read(&backup)? == entry.before,
                    "skills backup changed"
                );
                ensure!(
                    observed == entry.after || observed == Snapshot::Absent,
                    "skills recovery conflict"
                );
                snapshot::remove(&live, &observed)?;
                fs::rename(&backup, &live)?;
            } else if entry.before == Snapshot::Absent {
                ensure!(
                    observed == entry.after || observed == Snapshot::Absent,
                    "skills recovery conflict"
                );
                snapshot::remove(&live, &observed)?;
            } else {
                ensure!(
                    observed == entry.before,
                    "skills original missing during recovery"
                );
            }
            snapshot::sync_dir(live.parent().expect("validated artifact parent"))?;
        }
        Ok(())
    }
    pub fn cleanup(&self, root: &Path, cache: &Path) -> Result<()> {
        self.validate_slots(root)?;
        for (index, entry) in self.entries.iter().enumerate() {
            for (role, expected) in [("stage", &entry.after), ("backup", &entry.before)] {
                let path = entry.slot(root, &self.transaction_id, index, role);
                if snapshot::read(&path)? != Snapshot::Absent {
                    snapshot::remove(&path, expected)?;
                    snapshot::sync_dir(path.parent().expect("validated artifact parent"))?;
                }
            }
        }
        fs::remove_file(cache.join("pending.json"))?;
        snapshot::sync_dir(cache)
    }
}
