//! Descriptor-bound snapshot opening and one-read validation.

use std::path::{Component, Path};

use super::{AgentObservationError, MAX_SNAPSHOT_BYTES};

const SNAPSHOT_READ_BYTES: usize = MAX_SNAPSHOT_BYTES + 1;

#[cfg(unix)]
use std::os::fd::RawFd;

#[cfg(unix)]
use nix::{
    errno::Errno,
    fcntl::{OFlag, open, openat},
    sys::stat::{Mode, SFlag, fstat},
    unistd::{close, read},
};

#[cfg(unix)]
struct OwnedDescriptor(RawFd);

#[cfg(unix)]
impl OwnedDescriptor {
    const fn raw(&self) -> RawFd {
        self.0
    }
}

#[cfg(unix)]
impl Drop for OwnedDescriptor {
    fn drop(&mut self) {
        let _ = close(self.0);
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SnapshotFileFacts {
    device: nix::libc::dev_t,
    inode: nix::libc::ino_t,
    mode: nix::libc::mode_t,
    length: u64,
    regular: bool,
}

#[cfg(unix)]
impl SnapshotFileFacts {
    fn from_descriptor(descriptor: RawFd) -> Result<Self, AgentObservationError> {
        let stat = fstat(descriptor).map_err(|_| AgentObservationError::TruncatedSnapshot)?;
        Ok(Self {
            device: stat.st_dev,
            inode: stat.st_ino,
            mode: stat.st_mode,
            length: u64::try_from(stat.st_size)
                .map_err(|_| AgentObservationError::TruncatedSnapshot)?,
            regular: SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFREG),
        })
    }

    fn validate(self) -> Result<(), AgentObservationError> {
        if !self.regular {
            return Err(AgentObservationError::InvalidFileType);
        }
        if self.mode & 0o077 != 0 {
            return Err(AgentObservationError::InvalidPermissions);
        }
        if self.length > u64::try_from(MAX_SNAPSHOT_BYTES).expect("snapshot byte bound fits u64") {
            return Err(AgentObservationError::SnapshotTooLarge);
        }
        Ok(())
    }
}

#[cfg(unix)]
pub(super) fn read_snapshot_once(path: &Path) -> Result<Option<Vec<u8>>, AgentObservationError> {
    read_snapshot_once_with_open_hook(path, || {})
}

#[cfg(unix)]
pub(super) fn read_snapshot_once_with_open_hook(
    path: &Path,
    before_file_open: impl FnOnce(),
) -> Result<Option<Vec<u8>>, AgentObservationError> {
    let layout = SnapshotPathLayout::parse(path)?;
    let canonical_home =
        std::fs::canonicalize(layout.home).map_err(|_| AgentObservationError::TruncatedSnapshot)?;
    let mut directory = open_canonical_directory(&canonical_home)?;
    for component in layout.cache_components {
        directory = match open_directory_at(&directory, component) {
            Ok(directory) => directory,
            Err(Errno::ENOENT) => return Ok(None),
            Err(error) => return Err(open_error(error)),
        };
    }
    before_file_open();
    let descriptor = match openat(
        Some(directory.raw()),
        layout.file_name,
        file_flags(),
        Mode::empty(),
    ) {
        Ok(descriptor) => OwnedDescriptor(descriptor),
        Err(Errno::ENOENT) => return Ok(None),
        Err(error) => return Err(open_error(error)),
    };
    read_opened_snapshot(&descriptor).map(Some)
}

#[cfg(unix)]
fn read_opened_snapshot(descriptor: &OwnedDescriptor) -> Result<Vec<u8>, AgentObservationError> {
    let before = SnapshotFileFacts::from_descriptor(descriptor.raw())?;
    read_validated_snapshot(
        before,
        |buffer| {
            read(descriptor.raw(), buffer).map_err(|_| AgentObservationError::TruncatedSnapshot)
        },
        || SnapshotFileFacts::from_descriptor(descriptor.raw()),
    )
}

#[cfg(unix)]
fn read_validated_snapshot(
    before: SnapshotFileFacts,
    read_once: impl FnOnce(&mut [u8]) -> Result<usize, AgentObservationError>,
    facts_after_read: impl FnOnce() -> Result<SnapshotFileFacts, AgentObservationError>,
) -> Result<Vec<u8>, AgentObservationError> {
    before.validate()?;
    let mut buffer = [0_u8; SNAPSHOT_READ_BYTES];
    let bytes_read = read_once(&mut buffer)?;
    if bytes_read > MAX_SNAPSHOT_BYTES {
        return Err(AgentObservationError::SnapshotTooLarge);
    }
    let after = facts_after_read()?;
    if before != after
        || u64::try_from(bytes_read).map_err(|_| AgentObservationError::TruncatedSnapshot)?
            != before.length
    {
        return Err(AgentObservationError::TruncatedSnapshot);
    }
    Ok(buffer[..bytes_read].to_vec())
}

#[cfg(all(test, unix))]
pub(super) fn read_opened_snapshot_for_test(
    body: &[u8],
    declared_length: usize,
    bytes_read: usize,
) -> Result<Vec<u8>, AgentObservationError> {
    let facts = SnapshotFileFacts {
        device: 1,
        inode: 1,
        mode: 0o600,
        length: u64::try_from(declared_length)
            .map_err(|_| AgentObservationError::TruncatedSnapshot)?,
        regular: true,
    };
    read_validated_snapshot(
        facts,
        |buffer| {
            let copied = body.len().min(bytes_read).min(buffer.len());
            buffer[..copied].copy_from_slice(&body[..copied]);
            Ok(bytes_read)
        },
        || Ok(facts),
    )
}

#[cfg(unix)]
struct SnapshotPathLayout<'a> {
    home: &'a Path,
    cache_components: [&'a std::ffi::OsStr; 5],
    file_name: &'a std::ffi::OsStr,
}

#[cfg(unix)]
impl<'a> SnapshotPathLayout<'a> {
    fn parse(path: &'a Path) -> Result<Self, AgentObservationError> {
        let file_name = path.file_name().ok_or(AgentObservationError::WrongPath)?;
        let observations = path.parent().ok_or(AgentObservationError::WrongPath)?;
        let workspace = observations
            .parent()
            .ok_or(AgentObservationError::WrongPath)?;
        let workspaces = workspace.parent().ok_or(AgentObservationError::WrongPath)?;
        let brain = workspaces
            .parent()
            .ok_or(AgentObservationError::WrongPath)?;
        let cache = brain.parent().ok_or(AgentObservationError::WrongPath)?;
        let home = cache.parent().ok_or(AgentObservationError::WrongPath)?;
        if observations.file_name() != Some(std::ffi::OsStr::new("receiver-observations"))
            || workspaces.file_name() != Some(std::ffi::OsStr::new("workspaces"))
            || brain.file_name() != Some(std::ffi::OsStr::new("brain"))
            || cache.file_name() != Some(std::ffi::OsStr::new(".cache"))
        {
            return Err(AgentObservationError::WrongPath);
        }
        Ok(Self {
            home,
            cache_components: [
                cache.file_name().expect("validated cache component"),
                brain.file_name().expect("validated brain component"),
                workspaces
                    .file_name()
                    .expect("validated workspaces component"),
                workspace
                    .file_name()
                    .ok_or(AgentObservationError::WrongPath)?,
                observations
                    .file_name()
                    .expect("validated observations component"),
            ],
            file_name,
        })
    }
}

#[cfg(unix)]
fn open_canonical_directory(path: &Path) -> Result<OwnedDescriptor, AgentObservationError> {
    if !path.is_absolute() {
        return Err(AgentObservationError::WrongPath);
    }
    let mut descriptor = OwnedDescriptor(
        open(Path::new("/"), directory_flags(), Mode::empty()).map_err(open_error)?,
    );
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                descriptor = open_directory_at(&descriptor, name).map_err(open_error)?;
            }
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(AgentObservationError::WrongPath);
            }
        }
    }
    Ok(descriptor)
}

#[cfg(unix)]
fn open_directory_at(
    parent: &OwnedDescriptor,
    name: &std::ffi::OsStr,
) -> Result<OwnedDescriptor, Errno> {
    openat(
        Some(parent.raw()),
        Path::new(name),
        directory_flags(),
        Mode::empty(),
    )
    .map(OwnedDescriptor)
}

#[cfg(unix)]
fn directory_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_DIRECTORY
}

#[cfg(unix)]
fn file_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_NOCTTY
}

#[cfg(unix)]
fn open_error(error: Errno) -> AgentObservationError {
    match error {
        Errno::ELOOP | Errno::ENOTDIR => AgentObservationError::InvalidFileType,
        Errno::EACCES | Errno::EPERM => AgentObservationError::InvalidPermissions,
        _ => AgentObservationError::TruncatedSnapshot,
    }
}

#[cfg(not(unix))]
pub(super) fn read_snapshot_once(path: &Path) -> Result<Option<Vec<u8>>, AgentObservationError> {
    if path.exists() {
        Err(AgentObservationError::InvalidPermissions)
    } else {
        Ok(None)
    }
}

#[cfg(all(test, not(unix)))]
pub(super) fn read_snapshot_once_with_open_hook(
    path: &Path,
    before_file_open: impl FnOnce(),
) -> Result<Option<Vec<u8>>, AgentObservationError> {
    before_file_open();
    read_snapshot_once(path)
}
