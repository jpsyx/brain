//! Descriptor-relative removal for exact workspace-cache files.

use std::path::Path;

#[cfg(unix)]
mod quarantine;

#[cfg(unix)]
use quarantine::{
    blocked_cleanup_marker_exists, create_quarantine, fail_closed, mark_cleanup_blocked,
    recover_bound_quarantines, remove_empty_quarantine,
};

#[cfg(all(test, unix))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SecureRemoveTestBoundary {
    OpenBeforeEntryStat,
    EntryIdentityVerifiedBeforeRename,
    QuarantineRenameBeforeVerification,
    QuarantineIdentityVerified,
    RenameMissingBeforeAbsenceCheck,
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
    sys::stat::{Mode, SFlag, fchmod, fstat, fstatat},
    unistd::{UnlinkatFlags, close, unlinkat},
};

#[cfg(unix)]
pub(super) struct OwnedDescriptor(RawFd);

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
    if recover_bound_quarantines(parent, name)? {
        return Ok(());
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
    observe_test_boundary(SecureRemoveTestBoundary::OpenBeforeEntryStat, relative);
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
        mark_cleanup_blocked(parent, name)?;
        return Err(identity_changed());
    }
    #[cfg(test)]
    observe_test_boundary(
        SecureRemoveTestBoundary::EntryIdentityVerifiedBeforeRename,
        relative,
    );
    #[cfg(not(test))]
    observe_test_boundary(relative);
    let quarantine = create_quarantine(parent, name)?;
    match renameat(
        Some(parent.raw()),
        Path::new(name),
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
    ) {
        Ok(()) => {}
        Err(Errno::ENOENT) => {
            #[cfg(test)]
            observe_test_boundary(
                SecureRemoveTestBoundary::RenameMissingBeforeAbsenceCheck,
                relative,
            );
            #[cfg(not(test))]
            observe_test_boundary(relative);
            return if exact_name_is_absent(parent, name)? {
                remove_empty_quarantine(parent, &quarantine)
            } else {
                fail_closed(parent, name, &quarantine)?;
                Err(identity_changed())
            };
        }
        Err(error) => {
            let _ = remove_empty_quarantine(parent, &quarantine);
            return Err(io_error(error));
        }
    }
    #[cfg(test)]
    observe_test_boundary(
        SecureRemoveTestBoundary::QuarantineRenameBeforeVerification,
        relative,
    );
    #[cfg(not(test))]
    observe_test_boundary(relative);
    let quarantined = match openat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        file_flags(),
        Mode::empty(),
    ) {
        Ok(descriptor) => OwnedDescriptor(descriptor),
        Err(error) => {
            fail_closed(parent, name, &quarantine)?;
            return Err(io_error(error));
        }
    };
    let moved = match fstat(quarantined.raw()) {
        Ok(moved) => moved,
        Err(error) => {
            fail_closed(parent, name, &quarantine)?;
            return Err(io_error(error));
        }
    };
    if moved.st_dev != opened.st_dev
        || moved.st_ino != opened.st_ino
        || !SFlag::from_bits_truncate(moved.st_mode).contains(SFlag::S_IFREG)
    {
        fail_closed(parent, name, &quarantine)?;
        return Err(identity_changed());
    }
    if let Err(error) = fchmod(quarantine.descriptor.raw(), Mode::empty()) {
        fail_closed(parent, name, &quarantine)?;
        return Err(io_error(error));
    }
    #[cfg(test)]
    observe_test_boundary(
        SecureRemoveTestBoundary::QuarantineIdentityVerified,
        relative,
    );
    #[cfg(not(test))]
    observe_test_boundary(relative);
    match exact_name_is_absent(parent, name) {
        Ok(true) => {}
        Ok(false) => {
            fail_closed(parent, name, &quarantine)?;
            return Err(identity_changed());
        }
        Err(error) => {
            fail_closed(parent, name, &quarantine)?;
            return Err(error);
        }
    }
    if let Err(error) = fchmod(quarantine.descriptor.raw(), Mode::from_bits_truncate(0o700)) {
        fail_closed(parent, name, &quarantine)?;
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
            fail_closed(parent, name, &quarantine)?;
            return Err(io_error(error));
        }
    }
    remove_empty_quarantine(parent, &quarantine)
}

#[cfg(unix)]
pub(super) fn exact_name_is_absent(
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
pub(super) fn identity_changed() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "workspace cleanup target identity changed",
    )
}

#[cfg(unix)]
pub(super) fn directory_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_DIRECTORY
}

#[cfg(unix)]
pub(super) fn file_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_NOCTTY
}

#[cfg(unix)]
pub(super) fn io_error(error: Errno) -> std::io::Error {
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
mod tests;
