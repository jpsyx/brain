use std::{
    fs::OpenOptions,
    io::Write as _,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};

use crate::{command::server::receiver::hooks, workspace::CommandContext};

pub(super) struct SetupSnapshot {
    providers: serde_json::Map<String, serde_json::Value>,
    /// The machine-global env before setup: the public receiver origin lives
    /// there, since one machine serves one URL per channel.
    machine_providers: serde_json::Map<String, serde_json::Value>,
    provider_writes: Vec<(&'static str, String)>,
    files: Vec<FileSnapshot>,
    directories: Vec<DirectorySnapshot>,
}

struct FileSnapshot {
    path: PathBuf,
    symlink_target: Option<PathBuf>,
    resolved_destination: PathBuf,
    bytes: Option<Vec<u8>>,
    written_bytes: Option<Vec<u8>>,
    #[cfg(unix)]
    mode: Option<u32>,
}

struct DirectorySnapshot {
    path: PathBuf,
    existed: bool,
}

impl SetupSnapshot {
    pub(super) fn capture(context: &CommandContext, home: &Path) -> Result<Self> {
        let root = context.workspace.root();
        let installations = hooks::lifecycle_installations();
        let files = std::iter::once(crate::users::UsersStore::path(&context.workspace))
            .chain(installations.iter().flat_map(|installation| {
                std::iter::once(installation.path(root, home))
                    .chain(installation.auxiliary_paths(root, home))
            }))
            .map(FileSnapshot::capture)
            .collect::<Result<Vec<_>>>()?;
        let mut directory_paths = std::collections::BTreeSet::new();
        for path in installations.iter().flat_map(|installation| {
            std::iter::once(installation.path(root, home))
                .chain(installation.auxiliary_paths(root, home))
        }) {
            let base = if path.starts_with(root) { root } else { home };
            let mut parent = path.parent();
            while let Some(directory) = parent.filter(|directory| *directory != base) {
                directory_paths.insert(directory.to_path_buf());
                parent = directory.parent();
            }
        }
        let mut directory_paths = directory_paths.into_iter().collect::<Vec<_>>();
        directory_paths.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        let directories = directory_paths
            .into_iter()
            .map(|path| DirectorySnapshot {
                existed: path.is_dir(),
                path,
            })
            .collect();
        Ok(Self {
            providers: crate::env::load_map(context),
            machine_providers: crate::env::load_global_map(context),
            provider_writes: Vec::new(),
            files,
            directories,
        })
    }

    pub(super) fn restore(&self, context: &CommandContext) -> Result<()> {
        let mut failures = Vec::new();
        for file in self.files.iter().rev() {
            if let Err(error) = file.restore() {
                failures.push(format!("restore {}: {error:#}", file.path.display()));
            }
        }
        if let Err(error) =
            crate::env::restore_values_if_unchanged(context, &self.providers, &self.provider_writes)
        {
            failures.push(format!("restore selected provider record: {error:#}"));
        }
        if let Err(error) = crate::env::restore_global_values_if_unchanged(
            context,
            &self.machine_providers,
            &self.provider_writes,
        ) {
            failures.push(format!("restore machine provider env: {error:#}"));
        }
        for directory in &self.directories {
            if directory.existed {
                continue;
            }
            match std::fs::remove_dir(&directory.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(error) => failures.push(format!(
                    "remove rollback directory {}: {error}",
                    directory.path.display()
                )),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }

    pub(super) fn record_providers(&mut self, providers: &[(&'static str, String)]) {
        self.provider_writes = providers.to_vec();
    }

    pub(super) fn record_file(&mut self, path: &Path) -> Result<()> {
        let Some(snapshot) = self.files.iter_mut().find(|file| file.path == path) else {
            return Ok(());
        };
        snapshot.written_bytes = Some(
            std::fs::read(path).with_context(|| format!("record write to {}", path.display()))?,
        );
        Ok(())
    }

    pub(super) fn record_hook_step(
        &mut self,
        root: &Path,
        home: &Path,
        installation: crate::agent::LifecycleInstallation,
    ) -> Result<()> {
        self.record_file(&installation.path(root, home))?;
        for auxiliary in installation.auxiliary_paths(root, home) {
            self.record_file(&auxiliary)?;
        }
        Ok(())
    }
}

impl FileSnapshot {
    fn capture(path: PathBuf) -> Result<Self> {
        let symlink_target = match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => Some(
                std::fs::read_link(&path)
                    .with_context(|| format!("read snapshot symlink {}", path.display()))?,
            ),
            Ok(_) => None,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("inspect snapshot {}", path.display()));
            }
        };
        let resolved_destination = if symlink_target.is_some() {
            resolve_symlink_chain(&path)?
        } else {
            path.clone()
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("snapshot {}", path.display()));
            }
        };
        #[cfg(unix)]
        let mode = match std::fs::metadata(&path) {
            Ok(metadata) => {
                use std::os::unix::fs::PermissionsExt as _;
                Some(metadata.permissions().mode() & 0o777)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("snapshot permissions for {}", path.display()));
            }
        };
        Ok(Self {
            path,
            symlink_target,
            resolved_destination,
            bytes,
            written_bytes: None,
            #[cfg(unix)]
            mode,
        })
    }

    fn restore(&self) -> Result<()> {
        let Some(written) = &self.written_bytes else {
            return Ok(());
        };
        let Some(destination) = self.owned_destination()? else {
            return Ok(());
        };
        match std::fs::read(&destination) {
            Ok(current) if current != *written => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| format!("read {}", self.path.display()));
            }
            Ok(_) => {}
        }
        let Some(bytes) = &self.bytes else {
            return match std::fs::remove_file(&destination) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => {
                    Err(error).with_context(|| format!("remove {}", destination.display()))
                }
            };
        };
        replace_file(
            &destination,
            bytes,
            #[cfg(unix)]
            self.mode.unwrap_or(0o600),
        )
    }

    fn owned_destination(&self) -> Result<Option<PathBuf>> {
        match &self.symlink_target {
            Some(expected) => {
                let metadata = match std::fs::symlink_metadata(&self.path) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(error) => {
                        return Err(error)
                            .with_context(|| format!("inspect {}", self.path.display()));
                    }
                };
                if !metadata.file_type().is_symlink()
                    || std::fs::read_link(&self.path).ok().as_ref() != Some(expected)
                {
                    return Ok(None);
                }
                let current_destination = resolve_symlink_chain(&self.path)?;
                if current_destination != self.resolved_destination {
                    return Ok(None);
                }
                Ok(Some(self.resolved_destination.clone()))
            }
            None => match std::fs::symlink_metadata(&self.path) {
                Ok(metadata) if metadata.file_type().is_symlink() => Ok(None),
                Ok(_) => Ok(Some(self.path.clone())),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(Some(self.path.clone()))
                }
                Err(error) => {
                    Err(error).with_context(|| format!("inspect {}", self.path.display()))
                }
            },
        }
    }
}

pub(super) fn resolve_symlink_chain(path: &Path) -> Result<PathBuf> {
    let mut current = path.to_path_buf();
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..64 {
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(current),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect symlink chain {}", current.display()));
            }
        };
        if !metadata.file_type().is_symlink() {
            return Ok(current);
        }
        anyhow::ensure!(
            visited.insert(current.clone()),
            "symlink cycle while snapshotting {}",
            path.display()
        );
        let target = std::fs::read_link(&current)
            .with_context(|| format!("read symlink {}", current.display()))?;
        current = if target.is_absolute() {
            target
        } else {
            current
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(target)
        };
    }
    anyhow::bail!(
        "symlink chain exceeds safe depth while snapshotting {}",
        path.display()
    )
}

fn replace_file(path: &Path, bytes: &[u8], #[cfg(unix)] mode: u32) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create rollback directory {}", parent.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let temporary = path.with_file_name(format!(".{file_name}.rollback-{}", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(mode);
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("create rollback file {}", temporary.display()))?;
    let result = (|| {
        file.write_all(bytes)
            .with_context(|| format!("write rollback file {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("sync rollback file {}", temporary.display()))?;
        drop(file);
        std::fs::rename(&temporary, path).with_context(|| {
            format!(
                "replace {} from rollback file {}",
                path.display(),
                temporary.display()
            )
        })?;
        if let Ok(directory) = std::fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
