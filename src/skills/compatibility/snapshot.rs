//! Checked, source-preserving extension input shared by every renderer.
use super::{ExtensionProjection, ProjectionDirection, project_extensions};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
};

pub(crate) struct CheckedSources {
    pub projection: ExtensionProjection,
}

impl CheckedSources {
    pub(crate) fn capture(directory: Option<&Path>) -> Result<Self> {
        let Some(directory) = directory else {
            return Self::from_sources(&BTreeMap::new());
        };
        if !check_directory(directory)? {
            return Self::from_sources(&BTreeMap::new());
        }
        let mut paths = Vec::new();
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|ext| ext == "md") {
                paths.push(entry.path());
            }
        }
        Self::read_selected(directory, paths)
    }

    /// Consume bytes already captured by a checked installer source snapshot.
    pub(crate) fn from_snapshot(sources: &BTreeMap<String, Vec<u8>>) -> Result<Self> {
        let sources = sources
            .iter()
            .map(|(path, bytes)| Ok((path.clone(), std::str::from_utf8(bytes)?.to_owned())))
            .collect::<Result<BTreeMap<_, _>>>()?;
        Self::from_sources(&sources)
    }

    fn read_selected(directory: &Path, paths: Vec<PathBuf>) -> Result<Self> {
        let prefix = if directory
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|p| p == ".config")
        {
            PathBuf::from(".config")
                .join(directory.file_name().context("extension directory name")?)
        } else {
            PathBuf::from(directory.file_name().context("extension directory name")?)
        };
        let mut sources = BTreeMap::new();
        for path in paths {
            let identity = prefix
                .join(path.file_name().context("extension source name")?)
                .into_os_string()
                .into_string()
                .map_err(|_| anyhow::anyhow!("non-UTF-8 extension name"))?;
            let contents = read_source(&path).map_err(|error| {
                crate::skills::diagnostic::Diagnostic::source(&error, &identity)
            })?;
            ensure!(
                sources.insert(identity, contents).is_none(),
                "duplicate extension source"
            );
        }
        Self::from_sources(&sources)
    }

    fn from_sources(sources: &BTreeMap<String, String>) -> Result<Self> {
        Ok(Self {
            projection: project_extensions(sources, ProjectionDirection::Current)?,
        })
    }
}

fn read_source(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "extension must be a regular file"
    );
    ensure!(
        metadata.permissions().mode() & 0o444 != 0,
        "extension is unreadable"
    );
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)?;
    ensure!(
        file.metadata()?.is_file(),
        "extension changed type during snapshot"
    );
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

fn check_directory(directory: &Path) -> Result<bool> {
    let absolute = if directory.is_absolute() {
        directory.to_owned()
    } else {
        std::env::current_dir()?.join(directory)
    };
    let mut current = PathBuf::new();
    for (index, component) in absolute.components().enumerate() {
        ensure!(
            !matches!(component, Component::ParentDir),
            "extension source cannot contain parent traversal"
        );
        current.push(component);
        match fs::symlink_metadata(&current) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
            Ok(metadata) => ensure!(
                (metadata.is_dir() || (index <= 1 && fs::metadata(&current)?.is_dir()))
                    && (index <= 1 || !metadata.file_type().is_symlink()),
                "extensions source is not a real directory: {}",
                current.display()
            ),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
