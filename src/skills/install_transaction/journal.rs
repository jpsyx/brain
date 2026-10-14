//! Durable whole-install intent and authenticated artifact restoration.
use super::{
    InstallReceipt, InstallVersion, ManagedSkillManifest,
    snapshot::{self, Snapshot},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
    RollingBack,
    DisposingRollback,
    DisposingCommitted,
}

impl Journal {
    pub fn planned(
        identity: &super::InstallIdentity,
        previous: Option<Vec<u8>>,
        plan: &super::ownership::Plan,
    ) -> Result<Self> {
        let next = serde_json::to_vec(&plan.manifest)?;
        snapshot::check_state_bytes(&next)?;
        let mut journal = Self {
            phase: Phase::DisposingCommitted,
            users: plan.users.clone(),
            legacy: plan.legacy.clone(),
            previous_sha256: previous.as_ref().map(|bytes| snapshot::digest(bytes)),
            next_sha256: snapshot::digest(&next),
            schema_version: 1,
            workspace_id: identity.workspace_id.clone(),
            root: identity.root.clone(),
            transaction_id: uuid::Uuid::new_v4().to_string(),
            previous,
            next,
            entries: plan.entries.clone(),
        };
        // This is the longest serialized phase; later transitions cannot exceed this size.
        snapshot::check_state_bytes(&serde_json::to_vec(&journal)?)?;
        journal.phase = Phase::Prepared;
        Ok(journal)
    }
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
}
