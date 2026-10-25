//! Immutable, digest-addressed retained package evidence for older manifest readers.
use super::{
    BTreeMap, Deserialize, Entry, FileChange, InstallIdentity, Journal, LockedInstall,
    ManagedSkillManifest, PackageMetadata, Result, Serialize, Snapshot, ensure, planning, snapshot,
    validate_artifacts,
};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    schema_version: u32,
    workspace_id: String,
    root: PathBuf,
    transaction_id: String,
    previous: Option<Vec<u8>>,
    next: Vec<u8>,
    artifacts: BTreeMap<String, FileChange>,
    package: PackageMetadata,
}
impl Journal {
    pub(in crate::skills::install_transaction) fn archive_bytes(&self) -> Result<Option<Vec<u8>>> {
        self.retained_package
            .as_ref()
            .map(|package| {
                let bytes = serde_json::to_vec(&Archive {
                    schema_version: 1,
                    workspace_id: self.workspace_id.clone(),
                    root: self.root.clone(),
                    transaction_id: self.transaction_id.clone(),
                    previous: self.previous.clone(),
                    next: self.next.clone(),
                    artifacts: self.artifacts.clone(),
                    package: package.clone(),
                })?;
                snapshot::check_state_bytes(&bytes)?;
                Ok(bytes)
            })
            .transpose()
    }
    fn archive_path(&self, cache: &Path) -> Result<PathBuf> {
        let digest = self
            .archive_sha256
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing package archive digest"))?;
        ensure!(
            super::super::validation::hash(digest),
            "invalid package archive digest"
        );
        Ok(cache
            .join("package-backups")
            .join(format!("{}-{digest}.json", self.transaction_id)))
    }
    pub(in crate::skills::install_transaction) fn retain_package(
        &self,
        cache: &Path,
    ) -> Result<()> {
        let Some(bytes) = self.archive_bytes()? else {
            return Ok(());
        };
        ensure!(
            self.archive_sha256.as_deref() == Some(snapshot::digest(&bytes).as_str()),
            "package archive digest mismatch"
        );
        snapshot::ensure_parent(cache, "package-backups/archive.json")?;
        let path = self.archive_path(cache)?;
        ensure!(
            snapshot::read_optional_file(&path)?.is_none(),
            "package archive collision"
        );
        snapshot::atomic_write(&path, &bytes)
    }
    pub(in crate::skills::install_transaction) fn validate_archive(
        &self,
        cache: &Path,
    ) -> Result<()> {
        let expected = self.archive_bytes()?;
        ensure!(
            self.archive_sha256 == expected.as_ref().map(|bytes| snapshot::digest(bytes)),
            "package archive authentication mismatch"
        );
        if let Some(expected) = expected {
            snapshot::check_parent(cache, "package-backups/archive.json")?;
            ensure!(
                snapshot::read_optional_file(&self.archive_path(cache)?)? == Some(expected),
                "retained package evidence changed"
            );
        }
        Ok(())
    }
}

#[cfg_attr(not(test), allow(dead_code))] // Selection belongs to the subsequent package composer.
#[derive(Debug)]
pub(crate) struct RetainedPackage {
    pub transaction_id: String,
    pub previous: Option<ManagedSkillManifest>,
    pub target: ManagedSkillManifest,
    pub metadata: PackageMetadata,
    pub artifacts: BTreeMap<String, FileChange>,
}
impl LockedInstall {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn retained_packages(&self) -> Result<Vec<RetainedPackage>> {
        snapshot::check_parent(&self.identity.cache, "package-backups/archive.json")?;
        let directory = self.identity.cache.join("package-backups");
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut records = Vec::new();
        let mut budget = snapshot::STATE_FILE_LIMIT;
        for entry in entries {
            ensure!(records.len() < 4096, "too many retained package records");
            let entry = entry?;
            let bytes = snapshot::read_optional_file(&entry.path())?
                .ok_or_else(|| anyhow::anyhow!("retained package disappeared"))?;
            budget = budget
                .checked_sub(bytes.len())
                .ok_or_else(|| anyhow::anyhow!("retained package evidence exceeds read limit"))?;
            let archive: Archive = serde_json::from_slice(&bytes)?;
            ensure!(
                archive.schema_version == 1
                    && archive.root == self.identity.root
                    && archive.workspace_id == self.identity.workspace_id,
                "foreign retained package identity/schema"
            );
            ensure!(
                uuid::Uuid::parse_str(&archive.transaction_id)?.to_string()
                    == archive.transaction_id,
                "invalid retained transaction identity"
            );
            ensure!(
                entry.file_name().to_str()
                    == Some(
                        format!(
                            "{}-{}.json",
                            archive.transaction_id,
                            snapshot::digest(&bytes)
                        )
                        .as_str()
                    ),
                "retained package filename digest mismatch"
            );
            records.push(archive.validate(&self.identity)?);
        }
        records.sort_by(|a, b| a.transaction_id.cmp(&b.transaction_id));
        Ok(records)
    }
}

impl Archive {
    #[cfg_attr(not(test), allow(dead_code))]
    fn validate(self, identity: &InstallIdentity) -> Result<RetainedPackage> {
        let next: ManagedSkillManifest = serde_json::from_slice(&self.next)?;
        let previous: Option<ManagedSkillManifest> = self
            .previous
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()?;
        super::super::validation::manifest(&next, identity)?;
        if let Some(previous) = &previous {
            super::super::validation::manifest(previous, identity)?;
        }
        let entries = self
            .artifacts
            .iter()
            .map(|(path, change)| Entry {
                live: path.clone(),
                kind: Some(change.kind),
                before: planning::file_snapshot(change.before.as_deref(), &Snapshot::Absent),
                after: planning::file_snapshot(change.after.as_deref(), &Snapshot::Absent),
            })
            .collect();
        let journal = Journal {
            phase: super::super::journal::Phase::Prepared,
            users: BTreeMap::new(),
            legacy: BTreeMap::new(),
            previous_sha256: self.previous.as_ref().map(|bytes| snapshot::digest(bytes)),
            next_sha256: snapshot::digest(&self.next),
            schema_version: 2,
            workspace_id: self.workspace_id,
            root: self.root,
            transaction_id: self.transaction_id.clone(),
            previous: self.previous,
            next: self.next,
            entries,
            artifacts: self.artifacts.clone(),
            retained_package: Some(self.package.clone()),
            archive_sha256: None,
        };
        validate_artifacts(&journal, &next, previous.as_ref())?;
        Ok(RetainedPackage {
            transaction_id: self.transaction_id,
            previous,
            target: next,
            metadata: self.package,
            artifacts: self.artifacts,
        })
    }
}
