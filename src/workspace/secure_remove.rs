//! Descriptor-relative removal for exact workspace-cache files.

use std::path::Path;

#[cfg(unix)]
use std::os::fd::RawFd;

#[cfg(unix)]
use nix::{
    errno::Errno,
    fcntl::{AtFlags, OFlag, open, openat},
    sys::stat::{Mode, SFlag, fstat, fstatat},
    unistd::{UnlinkatFlags, close, unlinkat},
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
pub(crate) fn remove_regular_file_beneath(root: &Path, relative: &Path) -> std::io::Result<()> {
    let root_name = root.file_name().ok_or_else(invalid_path)?;
    let root_parent = root.parent().ok_or_else(invalid_path)?;
    let parent = match open_absolute_directory(root_parent) {
        Ok(parent) => parent,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut directory = match open_directory_at(&parent, root_name) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let std::path::Component::Normal(name) = component else {
            return Err(invalid_path());
        };
        if components.peek().is_none() {
            return remove_regular_at(&directory, name);
        }
        directory = match open_directory_at(&directory, name) {
            Ok(next) => next,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
    }
    Err(invalid_path())
}

#[cfg(unix)]
fn open_absolute_directory(path: &Path) -> std::io::Result<OwnedDescriptor> {
    if !path.is_absolute() {
        return Err(invalid_path());
    }
    let mut descriptor =
        OwnedDescriptor(open(Path::new("/"), directory_flags(), Mode::empty()).map_err(io_error)?);
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => {
                descriptor = open_directory_at(&descriptor, name)?;
            }
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::Prefix(_) => return Err(invalid_path()),
        }
    }
    Ok(descriptor)
}

#[cfg(unix)]
fn open_directory_at(
    parent: &OwnedDescriptor,
    name: &std::ffi::OsStr,
) -> std::io::Result<OwnedDescriptor> {
    openat(
        Some(parent.raw()),
        Path::new(name),
        directory_flags(),
        Mode::empty(),
    )
    .map(OwnedDescriptor)
    .map_err(io_error)
}

#[cfg(unix)]
fn remove_regular_at(parent: &OwnedDescriptor, name: &std::ffi::OsStr) -> std::io::Result<()> {
    let descriptor = match openat(
        Some(parent.raw()),
        Path::new(name),
        file_flags(),
        Mode::empty(),
    ) {
        Ok(descriptor) => OwnedDescriptor(descriptor),
        Err(Errno::ENOENT) => return Ok(()),
        Err(error) => return Err(io_error(error)),
    };
    let opened = fstat(descriptor.raw()).map_err(io_error)?;
    if !SFlag::from_bits_truncate(opened.st_mode).contains(SFlag::S_IFREG) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workspace cleanup target is not a regular file",
        ));
    }
    let entry = fstatat(
        Some(parent.raw()),
        Path::new(name),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io_error)?;
    if entry.st_dev != opened.st_dev
        || entry.st_ino != opened.st_ino
        || !SFlag::from_bits_truncate(entry.st_mode).contains(SFlag::S_IFREG)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workspace cleanup target identity changed",
        ));
    }
    match unlinkat(
        Some(parent.raw()),
        Path::new(name),
        UnlinkatFlags::NoRemoveDir,
    ) {
        Ok(()) | Err(Errno::ENOENT) => Ok(()),
        Err(error) => Err(io_error(error)),
    }
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
fn io_error(error: Errno) -> std::io::Error {
    std::io::Error::from_raw_os_error(error as i32)
}

fn invalid_path() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "workspace cleanup path is invalid",
    )
}

#[cfg(not(unix))]
pub(crate) fn remove_regular_file_beneath(_root: &Path, _relative: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "descriptor-relative workspace cleanup is unavailable",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::symlink;

    use super::remove_regular_file_beneath;

    #[test]
    fn cleanup_rejects_a_symlink_above_the_cache_root() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let temporary_root =
            std::fs::canonicalize(temporary.path()).expect("canonical temporary directory");
        let cache_parent = temporary_root.join("workspace-caches");
        let cache_root = cache_parent.join("workspace-id");
        let original_target = cache_root.join("responses/instance.json");
        std::fs::create_dir_all(
            original_target
                .parent()
                .expect("original response directory"),
        )
        .expect("original cache tree");
        std::fs::write(&original_target, "original private artifact")
            .expect("original private artifact");

        let retained_parent = temporary_root.join("workspace-caches-real");
        std::fs::rename(&cache_parent, &retained_parent).expect("retain original cache parent");
        let outside_parent = temporary_root.join("outside");
        let outside_target = outside_parent.join("workspace-id/responses/instance.json");
        std::fs::create_dir_all(outside_target.parent().expect("outside response directory"))
            .expect("outside cache tree");
        std::fs::write(&outside_target, "outside private artifact")
            .expect("outside private artifact");
        symlink(&outside_parent, &cache_parent).expect("replace cache parent with symlink");

        remove_regular_file_beneath(&cache_root, std::path::Path::new("responses/instance.json"))
            .expect_err("cleanup must reject every symlinked ancestor");

        assert!(
            outside_target.exists(),
            "cleanup deleted an outside artifact through a higher ancestor symlink"
        );
    }
}
