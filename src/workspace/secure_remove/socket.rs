//! Descriptor-relative quarantine removal for one exact Unix socket identity.

use std::path::Path;

use nix::errno::Errno;
use nix::fcntl::{AtFlags, renameat};
use nix::sys::stat::{SFlag, fstat, fstatat};
use nix::unistd::{UnlinkatFlags, unlinkat};

use super::{
    blocked_cleanup_marker_exists, create_quarantine, exact_name_is_absent, fail_closed,
    identity_changed, io_error, open_absolute_directory, promote_quarantine,
    remove_empty_quarantine,
};

pub(crate) fn remove_socket_file_beneath(
    root: &Path,
    relative: &Path,
    expected_device: u64,
    expected_inode: u64,
    expected_uid: u32,
) -> std::io::Result<()> {
    let mut components = relative.components();
    let Some(std::path::Component::Normal(name)) = components.next() else {
        return Err(identity_changed());
    };
    if components.next().is_some() {
        return Err(identity_changed());
    }
    let parent = open_absolute_directory(root)?;
    let parent_stat = fstat(parent.raw()).map_err(io_error)?;
    if !SFlag::from_bits_truncate(parent_stat.st_mode).contains(SFlag::S_IFDIR)
        || parent_stat.st_uid != expected_uid
        || parent_stat.st_mode & 0o022 != 0
    {
        return Err(identity_changed());
    }
    if blocked_cleanup_marker_exists(&parent, name)? {
        return Err(identity_changed());
    }
    let entry = match fstatat(
        Some(parent.raw()),
        Path::new(name),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(entry) => entry,
        Err(Errno::ENOENT) => return Ok(()),
        Err(error) => return Err(io_error(error)),
    };
    if !matches_socket(entry, expected_device, expected_inode, expected_uid) {
        return Ok(());
    }
    #[cfg(test)]
    super::observe_test_boundary(
        super::SecureRemoveTestBoundary::EntryIdentityVerifiedBeforeRename,
        relative,
    );
    #[cfg(not(test))]
    super::observe_test_boundary(relative);
    let mut quarantine = create_quarantine(&parent, name)?;
    match renameat(
        Some(parent.raw()),
        Path::new(name),
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
    ) {
        Ok(()) => {}
        Err(Errno::ENOENT) if exact_name_is_absent(&parent, name)? => {
            return remove_empty_quarantine(&parent, &quarantine);
        }
        Err(error) => {
            let _ = remove_empty_quarantine(&parent, &quarantine);
            return Err(io_error(error));
        }
    }
    #[cfg(test)]
    super::observe_test_boundary(
        super::SecureRemoveTestBoundary::QuarantineRenameBeforeVerification,
        relative,
    );
    #[cfg(not(test))]
    super::observe_test_boundary(relative);
    if let Err(error) = promote_quarantine(&parent, &mut quarantine) {
        fail_closed(&parent, name, &quarantine)?;
        return Err(error);
    }
    let moved = match fstatat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(moved) => moved,
        Err(error) => {
            fail_closed(&parent, name, &quarantine)?;
            return Err(io_error(error));
        }
    };
    if !matches_socket(moved, expected_device, expected_inode, expected_uid) {
        fail_closed(&parent, name, &quarantine)?;
        return Err(identity_changed());
    }
    let final_entry = fstatat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io_error)?;
    if !matches_socket(final_entry, expected_device, expected_inode, expected_uid) {
        fail_closed(&parent, name, &quarantine)?;
        return Err(identity_changed());
    }
    unlinkat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        UnlinkatFlags::NoRemoveDir,
    )
    .map_err(io_error)?;
    remove_empty_quarantine(&parent, &quarantine)
}

#[expect(
    clippy::verbose_bit_mask,
    reason = "the POSIX group-and-other permission mask states this ownership check directly"
)]
fn matches_socket(
    metadata: nix::libc::stat,
    expected_device: u64,
    expected_inode: u64,
    expected_uid: u32,
) -> bool {
    SFlag::from_bits_truncate(metadata.st_mode).contains(SFlag::S_IFSOCK)
        && device_matches(metadata.st_dev, expected_device)
        && metadata.st_ino == expected_inode
        && metadata.st_uid == expected_uid
        && metadata.st_mode & 0o077 == 0
}

#[cfg(target_os = "macos")]
fn device_matches(actual: nix::libc::dev_t, expected: u64) -> bool {
    u64::from(u32::from_ne_bytes(actual.to_ne_bytes())) == expected
}

#[cfg(not(target_os = "macos"))]
fn device_matches(actual: nix::libc::dev_t, expected: u64) -> bool {
    actual == expected
}
