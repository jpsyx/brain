//! Descriptor-relative removal for exact workspace-cache files.

use std::path::Path;

#[cfg(all(test, unix))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SecureRemoveTestBoundary {
    AfterOpenBeforeEntryStat,
    AfterQuarantineIdentityVerified,
}

#[cfg(all(test, unix))]
type SecureRemoveTestHook = Box<dyn FnMut(SecureRemoveTestBoundary, &Path)>;

#[cfg(all(test, unix))]
thread_local! {
    static SECURE_REMOVE_TEST_HOOK: std::cell::RefCell<Option<SecureRemoveTestHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, unix))]
pub(crate) fn with_secure_remove_test_hook<T>(
    hook: impl FnMut(SecureRemoveTestBoundary, &Path) + 'static,
    operation: impl FnOnce() -> T,
) -> T {
    struct HookGuard;

    impl Drop for HookGuard {
        fn drop(&mut self) {
            SECURE_REMOVE_TEST_HOOK.with(|installed| {
                installed.replace(None);
            });
        }
    }

    SECURE_REMOVE_TEST_HOOK.with(|installed| {
        assert!(installed.replace(Some(Box::new(hook))).is_none());
    });
    let _guard = HookGuard;
    operation()
}

#[cfg(all(test, unix))]
fn observe_test_boundary(boundary: SecureRemoveTestBoundary, relative: &Path) {
    SECURE_REMOVE_TEST_HOOK.with(|installed| {
        if let Some(hook) = installed.borrow_mut().as_mut() {
            hook(boundary, relative);
        }
    });
}

#[cfg(all(not(test), unix))]
fn observe_test_boundary(_relative: &Path) {}

#[cfg(unix)]
use std::os::fd::RawFd;

#[cfg(unix)]
use nix::{
    errno::Errno,
    fcntl::{AtFlags, OFlag, open, openat, renameat},
    sys::stat::{Mode, SFlag, fchmod, fstat, fstatat, mkdirat},
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
            return remove_regular_at(&directory, name, relative);
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
fn remove_regular_at(
    parent: &OwnedDescriptor,
    name: &std::ffi::OsStr,
    relative: &Path,
) -> std::io::Result<()> {
    if blocked_cleanup_marker_exists(parent, name)? {
        return Err(identity_changed());
    }
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
    #[cfg(test)]
    observe_test_boundary(SecureRemoveTestBoundary::AfterOpenBeforeEntryStat, relative);
    #[cfg(not(test))]
    observe_test_boundary(relative);
    let entry = match fstatat(
        Some(parent.raw()),
        Path::new(name),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(entry) => entry,
        Err(Errno::ENOENT) => return Ok(()),
        Err(error) => return Err(io_error(error)),
    };
    if entry.st_dev != opened.st_dev
        || entry.st_ino != opened.st_ino
        || !SFlag::from_bits_truncate(entry.st_mode).contains(SFlag::S_IFREG)
    {
        mark_cleanup_blocked(parent, name);
        return Err(identity_changed());
    }
    let quarantine = create_quarantine(parent)?;
    match renameat(
        Some(parent.raw()),
        Path::new(name),
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
    ) {
        Ok(()) => {}
        Err(Errno::ENOENT) => {
            remove_empty_quarantine(parent, &quarantine)?;
            return if exact_name_is_absent(parent, name)? {
                Ok(())
            } else {
                Err(identity_changed())
            };
        }
        Err(error) => {
            let _ = remove_empty_quarantine(parent, &quarantine);
            return Err(io_error(error));
        }
    }
    let quarantined = match openat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        file_flags(),
        Mode::empty(),
    ) {
        Ok(descriptor) => OwnedDescriptor(descriptor),
        Err(error) => {
            fail_closed(parent, name, &quarantine);
            return Err(io_error(error));
        }
    };
    let moved = match fstat(quarantined.raw()) {
        Ok(moved) => moved,
        Err(error) => {
            fail_closed(parent, name, &quarantine);
            return Err(io_error(error));
        }
    };
    if moved.st_dev != opened.st_dev
        || moved.st_ino != opened.st_ino
        || !SFlag::from_bits_truncate(moved.st_mode).contains(SFlag::S_IFREG)
    {
        fail_closed(parent, name, &quarantine);
        return Err(identity_changed());
    }
    if let Err(error) = fchmod(quarantine.descriptor.raw(), Mode::empty()) {
        fail_closed(parent, name, &quarantine);
        return Err(io_error(error));
    }
    #[cfg(test)]
    observe_test_boundary(
        SecureRemoveTestBoundary::AfterQuarantineIdentityVerified,
        relative,
    );
    #[cfg(not(test))]
    observe_test_boundary(relative);
    match exact_name_is_absent(parent, name) {
        Ok(true) => {}
        Ok(false) => {
            fail_closed(parent, name, &quarantine);
            return Err(identity_changed());
        }
        Err(error) => {
            fail_closed(parent, name, &quarantine);
            return Err(error);
        }
    }
    if let Err(error) = fchmod(quarantine.descriptor.raw(), Mode::from_bits_truncate(0o700)) {
        fail_closed(parent, name, &quarantine);
        return Err(io_error(error));
    }
    match unlinkat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        UnlinkatFlags::NoRemoveDir,
    ) {
        Ok(()) => {}
        Err(Errno::ENOENT) if exact_name_is_absent(&quarantine.descriptor, "artifact")? => {}
        Err(error) => {
            fail_closed(parent, name, &quarantine);
            return Err(io_error(error));
        }
    }
    remove_empty_quarantine(parent, &quarantine)
}

#[cfg(unix)]
struct Quarantine {
    name: std::ffi::OsString,
    descriptor: OwnedDescriptor,
}

#[cfg(unix)]
fn create_quarantine(parent: &OwnedDescriptor) -> std::io::Result<Quarantine> {
    for _ in 0..8 {
        let name = std::ffi::OsString::from(format!(".brain-cleanup-{}", uuid::Uuid::new_v4()));
        match mkdirat(
            Some(parent.raw()),
            Path::new(&name),
            Mode::from_bits_truncate(0o700),
        ) {
            Ok(()) => match open_directory_at(parent, &name) {
                Ok(descriptor) => return Ok(Quarantine { name, descriptor }),
                Err(error) => {
                    let _ = unlinkat(
                        Some(parent.raw()),
                        Path::new(&name),
                        UnlinkatFlags::RemoveDir,
                    );
                    return Err(error);
                }
            },
            Err(Errno::EEXIST) => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "workspace cleanup quarantine unavailable",
    ))
}

#[cfg(unix)]
fn remove_empty_quarantine(
    parent: &OwnedDescriptor,
    quarantine: &Quarantine,
) -> std::io::Result<()> {
    fchmod(quarantine.descriptor.raw(), Mode::from_bits_truncate(0o700)).map_err(io_error)?;
    match unlinkat(
        Some(parent.raw()),
        Path::new(&quarantine.name),
        UnlinkatFlags::RemoveDir,
    ) {
        Ok(()) => Ok(()),
        Err(Errno::ENOENT) if exact_name_is_absent(parent, &quarantine.name)? => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(unix)]
fn fail_closed(parent: &OwnedDescriptor, name: &std::ffi::OsStr, quarantine: &Quarantine) {
    let _ = fchmod(quarantine.descriptor.raw(), Mode::empty());
    mark_cleanup_blocked(parent, name);
}

#[cfg(unix)]
fn mark_cleanup_blocked(parent: &OwnedDescriptor, name: &std::ffi::OsStr) {
    let marker = blocked_cleanup_marker(name);
    let _ = mkdirat(Some(parent.raw()), Path::new(&marker), Mode::empty());
}

#[cfg(unix)]
fn blocked_cleanup_marker_exists(
    parent: &OwnedDescriptor,
    name: &std::ffi::OsStr,
) -> std::io::Result<bool> {
    let marker = blocked_cleanup_marker(name);
    match fstatat(
        Some(parent.raw()),
        Path::new(&marker),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(_) => Ok(true),
        Err(Errno::ENOENT) => Ok(false),
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(unix)]
fn blocked_cleanup_marker(name: &std::ffi::OsStr) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStrExt as _;

    let identity = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, name.as_bytes());
    std::ffi::OsString::from(format!(".brain-cleanup-blocked-{identity}"))
}

#[cfg(unix)]
fn exact_name_is_absent(
    parent: &OwnedDescriptor,
    name: impl AsRef<std::ffi::OsStr>,
) -> std::io::Result<bool> {
    match fstatat(
        Some(parent.raw()),
        Path::new(name.as_ref()),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Err(Errno::ENOENT) => Ok(true),
        Ok(_) => Ok(false),
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(unix)]
fn identity_changed() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "workspace cleanup target identity changed",
    )
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
