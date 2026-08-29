use std::{ffi::OsString, os::unix::ffi::OsStrExt as _, path::Path};

mod phase;

use nix::{
    dir::Dir,
    errno::Errno,
    fcntl::{AtFlags, openat},
    sys::stat::{FchmodatFlags, Mode, SFlag, fchmod, fchmodat, fstat, fstatat, mkdirat},
    unistd::{UnlinkatFlags, dup, unlinkat},
};

#[cfg(test)]
use super::recovery_nofollow_chmod_unsupported;
use super::{
    OwnedDescriptor, exact_name_is_absent, file_flags, identity_changed, io_error,
    open_directory_at,
};
use phase::{BoundQuarantine, QuarantinePhase, parse_bound_name, pending_name};

const MAX_BOUND_QUARANTINES: usize = 8;

pub(super) struct Quarantine {
    pub(super) name: OsString,
    pub(super) descriptor: OwnedDescriptor,
    phase: QuarantinePhase,
}

pub(super) fn promote_quarantine(
    parent: &OwnedDescriptor,
    quarantine: &mut Quarantine,
) -> std::io::Result<()> {
    phase::promote(parent, quarantine)
}

pub(super) fn create_quarantine(
    parent: &OwnedDescriptor,
    target: &std::ffi::OsStr,
) -> std::io::Result<Quarantine> {
    let prefix = bound_quarantine_prefix(target);
    for _ in 0..MAX_BOUND_QUARANTINES {
        let name = pending_name(&prefix);
        match mkdirat(
            Some(parent.raw()),
            Path::new(&name),
            Mode::from_bits_truncate(0o700),
        ) {
            Ok(()) => {
                #[cfg(test)]
                super::observe_test_boundary(
                    super::SecureRemoveTestBoundary::QuarantineCreatedBeforeOpen,
                    Path::new(target),
                );
                match open_verified_quarantine(parent, &name) {
                    Ok(descriptor) => {
                        return Ok(Quarantine {
                            name,
                            descriptor,
                            phase: QuarantinePhase::Pending,
                        });
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(Errno::EEXIST) => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "workspace cleanup quarantine unavailable",
    ))
}

pub(super) fn recover_bound_quarantines(
    parent: &OwnedDescriptor,
    target: &std::ffi::OsStr,
) -> std::io::Result<bool> {
    let names = discover_bound_quarantines(parent, target)?;
    if names.is_empty() {
        return Ok(false);
    }
    let mut recovered_artifact = false;
    for bound in names {
        recovered_artifact |= recover_one(parent, target, bound)?;
    }
    if !discover_bound_quarantines(parent, target)?.is_empty() {
        mark_cleanup_blocked(parent, target)?;
        return Err(identity_changed());
    }
    match exact_name_is_absent(parent, target) {
        Ok(true) => Ok(recovered_artifact),
        Ok(false) if recovered_artifact => {
            mark_cleanup_blocked(parent, target)?;
            Err(identity_changed())
        }
        Ok(false) => Ok(false),
        Err(error) => Err(error),
    }
}

fn discover_bound_quarantines(
    parent: &OwnedDescriptor,
    target: &std::ffi::OsStr,
) -> std::io::Result<Vec<BoundQuarantine>> {
    let prefix = bound_quarantine_prefix(target);
    let duplicate = dup(parent.raw()).map_err(io_error)?;
    let mut directory = Dir::from_fd(duplicate).map_err(io_error)?;
    let mut names = Vec::new();
    for entry in directory.iter() {
        let entry = entry.map_err(io_error)?;
        let bytes = entry.file_name().to_bytes();
        if !bytes.starts_with(prefix.as_bytes()) {
            continue;
        }
        let Some(bound) = parse_bound_name(bytes, prefix.len()) else {
            mark_cleanup_blocked(parent, target)?;
            return Err(identity_changed());
        };
        names.push(bound);
        if names.len() > MAX_BOUND_QUARANTINES {
            mark_cleanup_blocked(parent, target)?;
            return Err(identity_changed());
        }
    }
    names.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(names)
}

fn recover_one(
    parent: &OwnedDescriptor,
    target: &std::ffi::OsStr,
    bound: BoundQuarantine,
) -> std::io::Result<bool> {
    let descriptor = match open_verified_quarantine(parent, &bound.name) {
        Ok(opened) => opened,
        Err(error) => {
            mark_cleanup_blocked(parent, target)?;
            return Err(error);
        }
    };
    let opened = fstat(descriptor.raw()).map_err(io_error)?;
    let quarantine = Quarantine {
        name: bound.name,
        descriptor,
        phase: bound.phase,
    };
    let artifact_entry = match fstatat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(entry) => entry,
        Err(Errno::ENOENT) => {
            if !exact_name_is_absent(parent, target)?
                && quarantine.phase != QuarantinePhase::Pending
            {
                fail_closed(parent, target, &quarantine)?;
                return Err(identity_changed());
            }
            if let Err(error) = remove_empty_quarantine(parent, &quarantine) {
                fail_closed(parent, target, &quarantine)?;
                return Err(error);
            }
            return Ok(false);
        }
        Err(error) => return Err(io_error(error)),
    };
    if !SFlag::from_bits_truncate(artifact_entry.st_mode).contains(SFlag::S_IFREG)
        || artifact_entry.st_uid != opened.st_uid
    {
        fail_closed(parent, target, &quarantine)?;
        return Err(identity_changed());
    }
    let mut quarantine = quarantine;
    if let Err(error) = promote_quarantine(parent, &mut quarantine) {
        fail_closed(parent, target, &quarantine)?;
        return Err(error);
    }
    let artifact = openat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        file_flags(),
        Mode::empty(),
    )
    .map(OwnedDescriptor)
    .map_err(io_error)?;
    let held = fstat(artifact.raw()).map_err(io_error)?;
    if held.st_dev != artifact_entry.st_dev || held.st_ino != artifact_entry.st_ino {
        fail_closed(parent, target, &quarantine)?;
        return Err(identity_changed());
    }
    if !exact_name_is_absent(parent, target)? {
        fail_closed(parent, target, &quarantine)?;
        return Err(identity_changed());
    }
    if !exact_name_is_absent(parent, target)? {
        fail_closed(parent, target, &quarantine)?;
        return Err(identity_changed());
    }
    let final_entry = match fstatat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(entry) => entry,
        Err(error) => {
            fail_closed(parent, target, &quarantine)?;
            return Err(io_error(error));
        }
    };
    if final_entry.st_dev != held.st_dev
        || final_entry.st_ino != held.st_ino
        || !SFlag::from_bits_truncate(final_entry.st_mode).contains(SFlag::S_IFREG)
    {
        fail_closed(parent, target, &quarantine)?;
        return Err(identity_changed());
    }
    unlinkat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        UnlinkatFlags::NoRemoveDir,
    )
    .map_err(io_error)?;
    match exact_name_is_absent(parent, target) {
        Ok(true) => {}
        Ok(false) => {
            fail_closed(parent, target, &quarantine)?;
            return Err(identity_changed());
        }
        Err(error) => {
            fail_closed(parent, target, &quarantine)?;
            return Err(error);
        }
    }
    remove_empty_quarantine(parent, &quarantine)?;
    Ok(true)
}

fn open_verified_quarantine(
    parent: &OwnedDescriptor,
    name: &std::ffi::OsStr,
) -> std::io::Result<OwnedDescriptor> {
    let parent_stat = fstat(parent.raw()).map_err(io_error)?;
    let expected = fstatat(
        Some(parent.raw()),
        Path::new(name),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io_error)?;
    if !SFlag::from_bits_truncate(expected.st_mode).contains(SFlag::S_IFDIR)
        || expected.st_uid != parent_stat.st_uid
    {
        return Err(identity_changed());
    }
    let descriptor = match open_directory_at(parent, name) {
        Ok(descriptor) => descriptor,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            chmod_quarantine_at(parent, name, FchmodatFlags::FollowSymlink)?;
            open_directory_at(parent, name)?
        }
        Err(error) => return Err(error),
    };
    let opened = fstat(descriptor.raw()).map_err(io_error)?;
    if opened.st_dev != expected.st_dev
        || opened.st_ino != expected.st_ino
        || !SFlag::from_bits_truncate(opened.st_mode).contains(SFlag::S_IFDIR)
        || opened.st_uid != parent_stat.st_uid
    {
        return Err(identity_changed());
    }
    fchmod(descriptor.raw(), Mode::from_bits_truncate(0o700)).map_err(io_error)?;
    let final_entry = fstatat(
        Some(parent.raw()),
        Path::new(name),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io_error)?;
    if final_entry.st_dev != opened.st_dev
        || final_entry.st_ino != opened.st_ino
        || !SFlag::from_bits_truncate(final_entry.st_mode).contains(SFlag::S_IFDIR)
        || final_entry.st_uid != parent_stat.st_uid
    {
        return Err(identity_changed());
    }
    Ok(descriptor)
}

fn chmod_quarantine_at(
    parent: &OwnedDescriptor,
    name: &std::ffi::OsStr,
    flags: FchmodatFlags,
) -> std::io::Result<()> {
    #[cfg(test)]
    if matches!(flags, FchmodatFlags::NoFollowSymlink) && recovery_nofollow_chmod_unsupported() {
        return Err(io_error(Errno::EOPNOTSUPP));
    }
    fchmodat(
        Some(parent.raw()),
        Path::new(name),
        Mode::from_bits_truncate(0o700),
        flags,
    )
    .map_err(io_error)
}

pub(super) fn remove_empty_quarantine(
    parent: &OwnedDescriptor,
    quarantine: &Quarantine,
) -> std::io::Result<()> {
    fchmod(quarantine.descriptor.raw(), Mode::from_bits_truncate(0o700)).map_err(io_error)?;
    let held = fstat(quarantine.descriptor.raw()).map_err(io_error)?;
    let entry = match fstatat(
        Some(parent.raw()),
        Path::new(&quarantine.name),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(entry) => entry,
        Err(Errno::ENOENT) if exact_name_is_absent(parent, &quarantine.name)? => return Ok(()),
        Err(error) => return Err(io_error(error)),
    };
    if entry.st_dev != held.st_dev
        || entry.st_ino != held.st_ino
        || !SFlag::from_bits_truncate(entry.st_mode).contains(SFlag::S_IFDIR)
    {
        return Err(identity_changed());
    }
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

pub(super) fn fail_closed(
    parent: &OwnedDescriptor,
    target: &std::ffi::OsStr,
    _quarantine: &Quarantine,
) -> std::io::Result<()> {
    mark_cleanup_blocked(parent, target)
}

pub(super) fn mark_cleanup_blocked(
    parent: &OwnedDescriptor,
    target: &std::ffi::OsStr,
) -> std::io::Result<()> {
    let marker = blocked_cleanup_marker(target);
    match mkdirat(Some(parent.raw()), Path::new(&marker), Mode::empty()) {
        Ok(()) | Err(Errno::EEXIST) => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

pub(super) fn blocked_cleanup_marker_exists(
    parent: &OwnedDescriptor,
    target: &std::ffi::OsStr,
) -> std::io::Result<bool> {
    let marker = blocked_cleanup_marker(target);
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

fn blocked_cleanup_marker(target: &std::ffi::OsStr) -> OsString {
    let identity = leaf_identity(target);
    OsString::from(format!(".brain-cleanup-blocked-{identity}"))
}

fn bound_quarantine_prefix(target: &std::ffi::OsStr) -> String {
    let identity = leaf_identity(target);
    format!(".brain-cleanup-{identity}-")
}

fn leaf_identity(target: &std::ffi::OsStr) -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, target.as_bytes())
}
