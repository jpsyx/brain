//! Complete, non-following fingerprints and copies of transaction artifacts.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Write, os::unix::fs::PermissionsExt, path::Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) enum Snapshot {
    Absent,
    File {
        sha256: String,
        mode: u32,
    },
    Link {
        target: String,
    },
    Directory {
        mode: u32,
        entries: BTreeMap<String, Self>,
    },
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn read(path: &Path) -> Result<Snapshot> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Snapshot::Absent),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        return Ok(Snapshot::Link {
            target: fs::read_link(path)?
                .into_os_string()
                .into_string()
                .map_err(|_| anyhow::anyhow!("non-UTF-8 symlink"))?,
        });
    }
    let mode = metadata.permissions().mode();
    if metadata.is_file() {
        return Ok(Snapshot::File {
            sha256: digest(&fs::read(path)?),
            mode,
        });
    }
    ensure!(
        metadata.is_dir(),
        "unsupported special file: {}",
        path.display()
    );
    let mut entries = BTreeMap::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("non-UTF-8 filename"))?;
        entries.insert(name, read(&entry.path())?);
    }
    Ok(Snapshot::Directory { mode, entries })
}

pub(super) fn copy(source: &Path, dest: &Path, expected: &Snapshot) -> Result<()> {
    ensure!(
        read(source)? == *expected,
        "source changed during skills install: {}",
        source.display()
    );
    match expected {
        Snapshot::Absent => {}
        Snapshot::File { mode, .. } => {
            fs::copy(source, dest)?;
            fs::set_permissions(dest, fs::Permissions::from_mode(*mode))?;
            fs::File::open(dest)?.sync_all()?;
        }
        Snapshot::Link { target } => std::os::unix::fs::symlink(target, dest)?,
        Snapshot::Directory { mode, entries } => {
            fs::create_dir(dest)?;
            for (name, child) in entries {
                copy(&source.join(name), &dest.join(name), child)?;
            }
            fs::set_permissions(dest, fs::Permissions::from_mode(*mode))?;
            sync_dir(dest)?;
        }
    }
    ensure!(
        read(dest)? == *expected,
        "skills backup verification failed"
    );
    Ok(())
}

pub(super) fn remove(path: &Path, expected: &Snapshot) -> Result<()> {
    ensure!(
        read(path)? == *expected,
        "recovery conflict at {}",
        path.display()
    );
    match expected {
        Snapshot::Absent => {}
        Snapshot::Directory { .. } => fs::remove_dir_all(path)?,
        _ => fs::remove_file(path)?,
    }
    Ok(())
}

pub(super) fn sync_dir(path: &Path) -> Result<()> {
    Ok(fs::File::open(path)?.sync_all()?)
}

pub(super) const STATE_FILE_LIMIT: usize = 16 * 1024 * 1024;

pub(super) fn check_state_bytes(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() <= STATE_FILE_LIMIT,
        "skills state exceeds the readable byte limit"
    );
    Ok(())
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    check_state_bytes(bytes)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing parent"))?;
    let temporary = parent.join(format!(".skills-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    sync_dir(parent)
}

pub(super) fn relative(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && !path.contains('\\'),
        "invalid relative skills path"
    );
    for part in path.split('/') {
        ensure!(
            !part.is_empty() && part != "." && part != "..",
            "invalid relative skills path"
        );
    }
    ensure!(!Path::new(path).is_absolute(), "absolute skills path");
    Ok(())
}

pub(super) fn ensure_parent(root: &Path, relative_path: &str) -> Result<()> {
    relative(relative_path)?;
    let mut current = root.to_path_buf();
    let parent = Path::new(relative_path)
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing parent"))?;
    for component in parent.components() {
        current.push(component);
        match read_metadata(&current)? {
            None => {
                fs::create_dir(&current)?;
                sync_dir(current.parent().expect("validated artifact parent"))?;
            }
            Some(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Some(_) => bail!("unsafe skills ancestor: {}", current.display()),
        }
    }
    Ok(())
}

fn read_metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn check_parent(root: &Path, relative_path: &str) -> Result<()> {
    relative(relative_path)?;
    let mut current = root.to_path_buf();
    for component in Path::new(relative_path)
        .parent()
        .expect("validated artifact parent")
        .components()
    {
        current.push(component);
        match read_metadata(&current)? {
            None => return Ok(()),
            Some(metadata) => ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "unsafe skills ancestor: {}",
                current.display()
            ),
        }
    }
    Ok(())
}

pub(super) fn materialize(
    root: &Path,
    dest: &Path,
    relative: &str,
    desired: &Snapshot,
    generated: &BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    match desired {
        Snapshot::Absent => {}
        Snapshot::File { mode, .. } => {
            if let Some(bytes) = generated.get(relative) {
                fs::write(dest, bytes)?;
            } else {
                fs::copy(root.join(relative), dest)?;
            }
            fs::set_permissions(dest, fs::Permissions::from_mode(*mode))?;
            fs::File::open(dest)?.sync_all()?;
        }
        Snapshot::Link { target } => std::os::unix::fs::symlink(target, dest)?,
        Snapshot::Directory { mode, entries } => {
            fs::create_dir(dest)?;
            for (name, child) in entries {
                materialize(
                    root,
                    &dest.join(name),
                    &format!("{relative}/{name}"),
                    child,
                    generated,
                )?;
            }
            fs::set_permissions(dest, fs::Permissions::from_mode(*mode))?;
            sync_dir(dest)?;
        }
    }
    ensure!(read(dest)? == *desired, "staged skill verification failed");
    Ok(())
}

pub(super) fn ensure_absolute_directory(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "skills cache must be absolute");
    let mut current = std::path::PathBuf::from("/");
    let mut components = path.components().skip(1);
    if let Some(first) = components.next() {
        current.push(first);
        if current.exists() {
            current = fs::canonicalize(current)?;
        } else {
            create_directory(&current)?;
        }
    }
    for component in components {
        ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "invalid skills cache path"
        );
        current.push(component);
        match read_metadata(&current)? {
            None => {
                create_directory(&current)?;
                sync_dir(current.parent().expect("validated artifact parent"))?;
            }
            Some(metadata) => ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "unsafe skills cache ancestor: {}",
                current.display()
            ),
        }
    }
    Ok(())
}

// A caller observed absence; another creator may have won before create_dir.
fn create_directory(path: &Path) -> Result<()> {
    validate_directory_creation(path, fs::create_dir(path))
}

fn validate_directory_creation(path: &Path, created: std::io::Result<()>) -> Result<()> {
    match created {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(
                read_metadata(path)?.is_some_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink()),
                "unsafe concurrent skills cache ancestor: {}", path.display()
            );
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn read_optional_file(path: &Path) -> Result<Option<Vec<u8>>> {
    match read_metadata(path)? {
        None => Ok(None),
        Some(metadata) => {
            ensure!(
                metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.len() <= STATE_FILE_LIMIT as u64,
                "invalid skills state file: {}",
                path.display()
            );
            Ok(Some(fs::read(path)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        STATE_FILE_LIMIT, atomic_write, create_directory, read_optional_file,
        validate_directory_creation,
    };

    #[test]
    fn concurrent_cache_directory_winner_is_revalidated_without_following_links() {
        let root = tempfile::tempdir().unwrap();
        let winner = root.path().join("directory");
        std::fs::create_dir(&winner).unwrap();
        assert!(
            create_directory(&winner).is_ok(),
            "a real concurrent directory winner is safe"
        );
        let file = root.path().join("file");
        std::fs::write(&file, b"preserve").unwrap();
        assert!(create_directory(&file).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"preserve");
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&winner, &link).unwrap();
        assert!(create_directory(&link).is_err());
        assert!(
            std::fs::symlink_metadata(link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn vanished_directory_winner_and_non_already_exists_errors_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let vanished = root.path().join("vanished");
        assert!(
            validate_directory_creation(&vanished, Err(std::io::ErrorKind::AlreadyExists.into()))
                .is_err()
        );
        assert!(!vanished.exists());
        let error = validate_directory_creation(
            root.path(),
            Err(std::io::ErrorKind::PermissionDenied.into()),
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        let missing_parent = root.path().join("absent/child");
        let error = create_directory(&missing_parent).unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::NotFound
        );
        let socket = root.path().join("socket");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert!(create_directory(&socket).is_err());
    }

    #[test]
    fn state_writer_and_reader_agree_at_the_exact_size_boundary() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.json");
        let accepted = vec![b'x'; STATE_FILE_LIMIT];
        atomic_write(&path, &accepted).unwrap();
        assert_eq!(read_optional_file(&path).unwrap().unwrap(), accepted);
        assert!(atomic_write(&path, &vec![b'y'; STATE_FILE_LIMIT + 1]).is_err());
        assert_eq!(read_optional_file(&path).unwrap().unwrap(), accepted);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
