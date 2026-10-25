//! Read-only currentness, sharing the installer's ownership checks.
use super::{InstallReceipt, InstallVersion, LockedInstall, ownership, snapshot};
use anyhow::{Result, ensure};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Currentness {
    Current(InstallReceipt),
    Repairable(Vec<String>),
    Outdated,
}

impl LockedInstall {
    /// Compare explicit source refresh against the same ownership plan used to publish.
    pub(crate) fn sources_current(
        &self,
        rendered: &super::RenderedInstall,
        version: &InstallVersion,
    ) -> Result<bool> {
        let previous = self.manifest()?;
        let package_missing = previous
            .as_ref()
            .map(|manifest| super::package::check_owned(&self.identity, manifest))
            .transpose()?
            .is_some_and(|missing| !missing.is_empty());
        let plan = ownership::plan(&self.identity, previous.as_ref(), rendered.clone(), version)?;
        Ok(!package_missing
            && previous.is_some_and(|manifest| manifest.files == plan.manifest.files)
            && plan.entries.iter().all(|entry| entry.before == entry.after))
    }

    pub(crate) fn inspect(&self, expected: &InstallVersion) -> Result<Currentness> {
        ensure!(
            snapshot::read_optional_file(&self.identity.cache.join("pending.json"))?.is_none(),
            "skills recovery required before inspection"
        );
        let Some(manifest) = self.manifest()? else {
            return Ok(Currentness::Outdated);
        };
        let mut missing = ownership::check_owned(&self.identity, &manifest)?;
        missing.extend(super::package::check_owned(&self.identity, &manifest)?);
        for name in ownership::roots(&manifest) {
            for frontend in [".claude", ".codex", ".opencode"] {
                let path = format!("{frontend}/skills/{name}");
                snapshot::check_parent(&self.identity.root, &path)?;
                let observed = snapshot::read(&self.identity.root.join(&path))?;
                if observed == snapshot::Snapshot::Absent {
                    missing.push(path);
                } else {
                    ensure!(
                        ownership::equivalent_link(&self.identity.root, &path, &observed, &name),
                        "protected frontend collision: {path}"
                    );
                    if observed
                        != (snapshot::Snapshot::Link {
                            target: format!("../../.agents/skills/{name}"),
                        })
                    {
                        missing.push(path);
                    }
                }
            }
        }
        if !missing.is_empty() {
            return Ok(Currentness::Repairable(missing));
        }
        if manifest.binary_version != expected.binary_version
            || manifest.compatibility_revision != expected.compatibility_revision
        {
            return Ok(Currentness::Outdated);
        }
        let bytes = snapshot::read_optional_file(&self.identity.cache.join("managed.json"))?
            .ok_or_else(|| anyhow::anyhow!("skills manifest disappeared"))?;
        Ok(Currentness::Current(InstallReceipt {
            version: expected.clone(),
            manifest_sha256: snapshot::digest(&bytes),
        }))
    }
}

#[derive(Debug)]
pub(crate) struct InstallBusy {
    pub workspace_id: String,
}
impl std::fmt::Display for InstallBusy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "skills install busy for {}", self.workspace_id)
    }
}
impl std::error::Error for InstallBusy {}
