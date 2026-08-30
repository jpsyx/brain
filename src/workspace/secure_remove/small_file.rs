//! Bounded descriptor-relative reads for one owner-controlled regular file.

use std::path::Path;

use nix::errno::Errno;
use nix::fcntl::{AtFlags, openat};
use nix::sys::stat::{SFlag, fstat, fstatat};
use nix::unistd::read;

use super::{OwnedDescriptor, VerifiedDirectory, file_flags, identity_changed, io_error};

pub(crate) fn read_small_owned_regular_file_in(
    parent: &VerifiedDirectory,
    relative: &Path,
    max_bytes: usize,
) -> std::io::Result<Option<Vec<u8>>> {
    let name = exact_leaf(relative)?;
    let expected_uid = parent.owner_uid();
    let parent = parent.descriptor();
    let descriptor = match openat(
        Some(parent.raw()),
        Path::new(name),
        file_flags(),
        nix::sys::stat::Mode::empty(),
    ) {
        Ok(descriptor) => OwnedDescriptor(descriptor),
        Err(Errno::ENOENT) => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    let opened = fstat(descriptor.raw()).map_err(io_error)?;
    if !matches_owned_regular(opened, expected_uid, max_bytes) {
        return Err(identity_changed());
    }
    verify_entry(parent, name, opened, expected_uid)?;
    let mut contents = vec![0_u8; max_bytes.checked_add(1).ok_or_else(identity_changed)?];
    let mut used = 0;
    while used < contents.len() {
        match read(descriptor.raw(), &mut contents[used..]) {
            Ok(0) => break,
            Ok(read) => used += read,
            Err(Errno::EINTR) => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    if used > max_bytes {
        return Err(identity_changed());
    }
    let held = fstat(descriptor.raw()).map_err(io_error)?;
    if held.st_dev != opened.st_dev || held.st_ino != opened.st_ino {
        return Err(identity_changed());
    }
    verify_entry(parent, name, held, expected_uid)?;
    contents.truncate(used);
    Ok(Some(contents))
}

fn exact_leaf(relative: &Path) -> std::io::Result<&std::ffi::OsStr> {
    let mut components = relative.components();
    let Some(std::path::Component::Normal(name)) = components.next() else {
        return Err(identity_changed());
    };
    if components.next().is_some() {
        return Err(identity_changed());
    }
    Ok(name)
}

fn matches_owned_regular(metadata: nix::libc::stat, expected_uid: u32, max_bytes: usize) -> bool {
    SFlag::from_bits_truncate(metadata.st_mode).contains(SFlag::S_IFREG)
        && metadata.st_uid == expected_uid
        && usize::try_from(metadata.st_size).is_ok_and(|size| size <= max_bytes)
}

fn verify_entry(
    parent: &OwnedDescriptor,
    name: &std::ffi::OsStr,
    opened: nix::libc::stat,
    expected_uid: u32,
) -> std::io::Result<()> {
    let entry = fstatat(
        Some(parent.raw()),
        Path::new(name),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io_error)?;
    if entry.st_dev != opened.st_dev
        || entry.st_ino != opened.st_ino
        || !SFlag::from_bits_truncate(entry.st_mode).contains(SFlag::S_IFREG)
        || entry.st_uid != expected_uid
    {
        return Err(identity_changed());
    }
    Ok(())
}
