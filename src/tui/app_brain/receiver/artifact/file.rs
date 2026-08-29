//! Descriptor-bound completion artifact opening and one-read validation.

use std::path::{Component, Path};

use super::{CompletionArtifactError, MAX_COMPLETION_ARTIFACT_BYTES};

const ARTIFACT_READ_BYTES: usize = MAX_COMPLETION_ARTIFACT_BYTES + 1;

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
struct ArtifactFileFacts {
    device: nix::libc::dev_t,
    inode: nix::libc::ino_t,
    mode: nix::libc::mode_t,
    length: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
    regular: bool,
}

#[cfg(unix)]
impl ArtifactFileFacts {
    fn from_descriptor(descriptor: RawFd) -> Result<Self, CompletionArtifactError> {
        let stat = fstat(descriptor).map_err(|_| CompletionArtifactError::Truncated)?;
        Ok(Self {
            device: stat.st_dev,
            inode: stat.st_ino,
            mode: stat.st_mode,
            length: u64::try_from(stat.st_size).map_err(|_| CompletionArtifactError::Truncated)?,
            modified_seconds: stat.st_mtime,
            modified_nanoseconds: stat.st_mtime_nsec,
            changed_seconds: stat.st_ctime,
            changed_nanoseconds: stat.st_ctime_nsec,
            regular: SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFREG),
        })
    }

    fn validate(self) -> Result<(), CompletionArtifactError> {
        if !self.regular {
            return Err(CompletionArtifactError::InvalidFileType);
        }
        if self.mode & 0o077 != 0 {
            return Err(CompletionArtifactError::InvalidPermissions);
        }
        if self.length
            > u64::try_from(MAX_COMPLETION_ARTIFACT_BYTES)
                .expect("completion artifact byte bound fits u64")
        {
            return Err(CompletionArtifactError::TooLarge);
        }
        Ok(())
    }
}

#[cfg(unix)]
pub(super) fn read_artifact_once(path: &Path) -> Result<Option<Vec<u8>>, CompletionArtifactError> {
    read_artifact_once_with_open_hook(path, || {})
}

#[cfg(all(test, unix))]
mod tests {
    use nix::sys::stat::{Mode, SFlag};
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    use super::{
        ArtifactFileFacts, CompletionArtifactError, read_artifact_once, read_validated_artifact,
    };

    fn facts(length: u64) -> ArtifactFileFacts {
        ArtifactFileFacts {
            device: 1,
            inode: 2,
            mode: SFlag::S_IFREG.bits() | 0o600,
            length,
            modified_seconds: 3,
            modified_nanoseconds: 4,
            changed_seconds: 5,
            changed_nanoseconds: 6,
            regular: true,
        }
    }

    #[test]
    fn one_read_rejects_short_reads_growth_and_identity_changes() {
        let short = read_validated_artifact(facts(4), |_| Ok(3), || Ok(facts(4)));
        assert_eq!(short, Err(CompletionArtifactError::Truncated));

        let growth = read_validated_artifact(facts(4), |_| Ok(4), || Ok(facts(5)));
        assert_eq!(growth, Err(CompletionArtifactError::Truncated));

        let replacement = read_validated_artifact(
            facts(4),
            |_| Ok(4),
            || {
                Ok(ArtifactFileFacts {
                    inode: 3,
                    ..facts(4)
                })
            },
        );
        assert_eq!(replacement, Err(CompletionArtifactError::Truncated));
    }

    #[test]
    fn one_read_rejects_same_length_in_place_rewrites() {
        let before = facts(4);
        let after = ArtifactFileFacts {
            modified_nanoseconds: before.modified_nanoseconds + 1,
            ..before
        };

        let rewritten = read_validated_artifact(before, |_| Ok(4), || Ok(after));

        assert_eq!(rewritten, Err(CompletionArtifactError::Truncated));
    }

    #[test]
    fn rejects_a_symlinked_cache_ancestor() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let home = temporary.path().join("home");
        let outside = temporary.path().join("outside");
        std::fs::create_dir_all(outside.join("brain/workspaces/workspace/responses"))
            .expect("outside responses");
        std::fs::create_dir_all(&home).expect("home");
        symlink(&outside, home.join(".cache")).expect("cache symlink");
        let path = home.join(".cache/brain/workspaces/workspace/responses/response.json");

        assert_eq!(
            read_artifact_once(&path),
            Err(CompletionArtifactError::InvalidFileType)
        );
    }

    #[test]
    fn rejects_a_fifo_without_blocking() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary
            .path()
            .join("home/.cache/brain/workspaces/workspace/responses/response.json");
        std::fs::create_dir_all(path.parent().expect("responses directory"))
            .expect("responses directory");
        nix::unistd::mkfifo(&path, Mode::from_bits_truncate(0o600)).expect("completion FIFO");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only FIFO");

        assert_eq!(
            read_artifact_once(&path),
            Err(CompletionArtifactError::InvalidFileType)
        );
    }
}

#[cfg(unix)]
fn read_artifact_once_with_open_hook(
    path: &Path,
    before_file_open: impl FnOnce(),
) -> Result<Option<Vec<u8>>, CompletionArtifactError> {
    let layout = ArtifactPathLayout::parse(path)?;
    let canonical_home =
        std::fs::canonicalize(layout.home).map_err(|_| CompletionArtifactError::Truncated)?;
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
    read_opened_artifact(&descriptor).map(Some)
}

#[cfg(unix)]
fn read_opened_artifact(descriptor: &OwnedDescriptor) -> Result<Vec<u8>, CompletionArtifactError> {
    let before = ArtifactFileFacts::from_descriptor(descriptor.raw())?;
    read_validated_artifact(
        before,
        |buffer| read(descriptor.raw(), buffer).map_err(|_| CompletionArtifactError::Truncated),
        || ArtifactFileFacts::from_descriptor(descriptor.raw()),
    )
}

#[cfg(unix)]
fn read_validated_artifact(
    before: ArtifactFileFacts,
    read_once: impl FnOnce(&mut [u8]) -> Result<usize, CompletionArtifactError>,
    facts_after_read: impl FnOnce() -> Result<ArtifactFileFacts, CompletionArtifactError>,
) -> Result<Vec<u8>, CompletionArtifactError> {
    before.validate()?;
    let mut buffer = vec![0_u8; ARTIFACT_READ_BYTES];
    let bytes_read = read_once(&mut buffer)?;
    if bytes_read > MAX_COMPLETION_ARTIFACT_BYTES {
        return Err(CompletionArtifactError::TooLarge);
    }
    let after = facts_after_read()?;
    if before != after
        || u64::try_from(bytes_read).map_err(|_| CompletionArtifactError::Truncated)?
            != before.length
    {
        return Err(CompletionArtifactError::Truncated);
    }
    buffer.truncate(bytes_read);
    Ok(buffer)
}

#[cfg(unix)]
struct ArtifactPathLayout<'a> {
    home: &'a Path,
    cache_components: [&'a std::ffi::OsStr; 5],
    file_name: &'a std::ffi::OsStr,
}

#[cfg(unix)]
impl<'a> ArtifactPathLayout<'a> {
    fn parse(path: &'a Path) -> Result<Self, CompletionArtifactError> {
        let file_name = path.file_name().ok_or(CompletionArtifactError::WrongPath)?;
        let responses = path.parent().ok_or(CompletionArtifactError::WrongPath)?;
        let workspace = responses
            .parent()
            .ok_or(CompletionArtifactError::WrongPath)?;
        let workspaces = workspace
            .parent()
            .ok_or(CompletionArtifactError::WrongPath)?;
        let brain = workspaces
            .parent()
            .ok_or(CompletionArtifactError::WrongPath)?;
        let cache = brain.parent().ok_or(CompletionArtifactError::WrongPath)?;
        let home = cache.parent().ok_or(CompletionArtifactError::WrongPath)?;
        if responses.file_name() != Some(std::ffi::OsStr::new("responses"))
            || workspaces.file_name() != Some(std::ffi::OsStr::new("workspaces"))
            || brain.file_name() != Some(std::ffi::OsStr::new("brain"))
            || cache.file_name() != Some(std::ffi::OsStr::new(".cache"))
        {
            return Err(CompletionArtifactError::WrongPath);
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
                    .ok_or(CompletionArtifactError::WrongPath)?,
                responses
                    .file_name()
                    .expect("validated responses component"),
            ],
            file_name,
        })
    }
}

#[cfg(unix)]
fn open_canonical_directory(path: &Path) -> Result<OwnedDescriptor, CompletionArtifactError> {
    if !path.is_absolute() {
        return Err(CompletionArtifactError::WrongPath);
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
                return Err(CompletionArtifactError::WrongPath);
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
fn open_error(error: Errno) -> CompletionArtifactError {
    match error {
        Errno::ELOOP | Errno::ENOTDIR => CompletionArtifactError::InvalidFileType,
        Errno::EACCES | Errno::EPERM => CompletionArtifactError::InvalidPermissions,
        _ => CompletionArtifactError::Truncated,
    }
}

#[cfg(not(unix))]
pub(super) fn read_artifact_once(path: &Path) -> Result<Option<Vec<u8>>, CompletionArtifactError> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Ok(metadata) if !metadata.file_type().is_file() || metadata.file_type().is_symlink() => {
            Err(CompletionArtifactError::InvalidFileType)
        }
        Ok(_) | Err(_) => Err(CompletionArtifactError::InvalidPermissions),
    }
}
