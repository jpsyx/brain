//! Validated workspace identity and a persistent advisory-lock inode.
use super::{LockedInstall, snapshot};
use crate::workspace::{WorkspaceContext, WorkspaceId, WorkspaceManifest, WorkspacePaths};
use anyhow::{Result, ensure};
use fs2::FileExt;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(crate) struct InstallIdentity {
    pub(super) root: PathBuf,
    pub(super) cache: PathBuf,
    pub(super) workspace_id: String,
    portable: bool,
}
impl InstallIdentity {
    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            fs::canonicalize(&self.root)? == self.root,
            "skills root identity changed"
        );
        ensure!(self.root.is_dir(), "skills root is not a directory");
        snapshot::check_parent(&self.root, ".config/workspace.json")?;
        if self.portable {
            ensure!(
                matches!(
                    snapshot::read(&WorkspaceManifest::path(&self.root))?,
                    snapshot::Snapshot::File { .. }
                ),
                "invalid portable skills identity"
            );
            let manifest = WorkspaceManifest::load(&self.root, super::super::current_version())?;
            ensure!(
                manifest.workspace_id().to_string() == self.workspace_id,
                "skills workspace identity changed"
            );
        } else {
            ensure!(
                snapshot::read(&WorkspaceManifest::path(&self.root))? == snapshot::Snapshot::Absent,
                "sandbox gained a portable identity; resolve it again"
            );
        }
        Ok(())
    }
    /// Validate/create the same cache ancestry before local migration recovery.
    pub(crate) fn prepare_cache(&self) -> Result<()> {
        snapshot::ensure_absolute_directory(&self.cache)
    }

    pub(crate) fn registered(root: &Path, id: WorkspaceId, cache: &Path) -> Result<Self> {
        let identity = Self::sandbox_with_cache(root, cache)?;
        let manifest = WorkspaceManifest::load(&identity.root, super::super::current_version())?;
        ensure!(
            manifest.workspace_id() == id,
            "skills workspace identity mismatch"
        );
        Ok(identity)
    }
    pub(crate) fn workspace(workspace: &WorkspaceContext) -> Result<Self> {
        Self::registered(
            workspace.root(),
            workspace.id(),
            workspace.paths().cache_dir(),
        )
    }
    pub(crate) fn sandbox_with_cache(root: &Path, cache: &Path) -> Result<Self> {
        let root = fs::canonicalize(root)?;
        ensure!(root.is_dir(), "skills root is not a directory");
        snapshot::check_parent(&root, ".config/workspace.json")?;
        let portable = snapshot::read(&WorkspaceManifest::path(&root))?;
        let workspace_id = if portable == snapshot::Snapshot::Absent {
            uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                root.as_os_str().as_encoded_bytes(),
            )
            .to_string()
        } else {
            ensure!(
                matches!(portable, snapshot::Snapshot::File { .. }),
                "invalid portable workspace identity file"
            );
            WorkspaceManifest::load(&root, super::super::current_version())?
                .workspace_id()
                .to_string()
        };
        Ok(Self {
            root,
            cache: cache.join("skills-install"),
            workspace_id,
            portable: portable != snapshot::Snapshot::Absent,
        })
    }
    pub(crate) fn sandbox(root: &Path) -> Result<Self> {
        let canonical = fs::canonicalize(root)?;
        let portable = snapshot::read(&WorkspaceManifest::path(&canonical))?;
        if portable == snapshot::Snapshot::Absent {
            return Self::sandbox_with_cache(&canonical, &canonical.join(".cache/brain"));
        }
        let manifest = WorkspaceManifest::load(&canonical, super::super::current_version())?;
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
        let paths = WorkspacePaths::new(&home, manifest.workspace_id());
        let store = crate::workspace::RegistryStore::real();
        let registry = crate::workspace::RegistryStore::load_readable(store.path())?;
        for record in registry.workspaces.values() {
            let same_root = record.root.canonicalize().ok().as_ref() == Some(&canonical);
            if same_root || record.workspace_id == manifest.workspace_id() {
                ensure!(
                    same_root && record.workspace_id == manifest.workspace_id(),
                    "sandbox workspace identity differs from registry"
                );
            }
        }
        Self::registered(&canonical, manifest.workspace_id(), paths.cache_dir())
    }
}

impl LockedInstall {
    pub(crate) fn acquire(identity: &InstallIdentity) -> Result<Self> {
        identity.validate()?;
        identity.prepare_cache()?;
        let lock_path = identity.cache.join("install.lock");
        ensure!(
            matches!(
                snapshot::read(&lock_path)?,
                snapshot::Snapshot::Absent | snapshot::Snapshot::File { .. }
            ),
            "unsafe skills lock file"
        );
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock.try_lock_exclusive()
            .map_err(|error| -> anyhow::Error {
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    super::InstallBusy {
                        workspace_id: identity.workspace_id.clone(),
                    }
                    .into()
                } else {
                    error.into()
                }
            })?;
        identity.validate()?;
        Ok(Self {
            identity: identity.clone(),
            _lock: lock,
        })
    }
}
