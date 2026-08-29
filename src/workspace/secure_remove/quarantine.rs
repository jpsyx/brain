use std::{
    ffi::OsString,
    os::unix::ffi::{OsStrExt as _, OsStringExt as _},
    path::Path,
};

use nix::{
    dir::Dir,
    errno::Errno,
    fcntl::{AtFlags, openat},
    sys::stat::{FchmodatFlags, Mode, SFlag, fchmod, fchmodat, fstat, fstatat, mkdirat},
    unistd::{UnlinkatFlags, dup, unlinkat},
};

use super::{
    OwnedDescriptor, exact_name_is_absent, file_flags, identity_changed, io_error,
    open_directory_at,
};

const MAX_BOUND_QUARANTINES: usize = 8;

pub(super) struct Quarantine {
    pub(super) name: OsString,
    pub(super) descriptor: OwnedDescriptor,
}

pub(super) fn create_quarantine(
    parent: &OwnedDescriptor,
    target: &std::ffi::OsStr,
) -> std::io::Result<Quarantine> {
    let prefix = bound_quarantine_prefix(target);
    for _ in 0..MAX_BOUND_QUARANTINES {
        let name = OsString::from(format!("{prefix}{}", uuid::Uuid::new_v4()));
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

pub(super) fn recover_bound_quarantines(
    parent: &OwnedDescriptor,
    target: &std::ffi::OsStr,
) -> std::io::Result<bool> {
    let names = discover_bound_quarantines(parent, target)?;
    if names.is_empty() {
        return Ok(false);
    }
    let mut recovered_artifact = false;
    for name in names {
        recovered_artifact |= recover_one(parent, target, &name)?;
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
) -> std::io::Result<Vec<OsString>> {
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
        let suffix = &bytes[prefix.len()..];
        let valid_random_uuid = std::str::from_utf8(suffix)
            .ok()
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
            .is_some_and(|value| value.get_version() == Some(uuid::Version::Random));
        if !valid_random_uuid {
            mark_cleanup_blocked(parent, target)?;
            return Err(identity_changed());
        }
        names.push(OsString::from_vec(bytes.to_vec()));
        if names.len() > MAX_BOUND_QUARANTINES {
            mark_cleanup_blocked(parent, target)?;
            return Err(identity_changed());
        }
    }
    names.sort();
    Ok(names)
}

fn recover_one(
    parent: &OwnedDescriptor,
    target: &std::ffi::OsStr,
    name: &std::ffi::OsStr,
) -> std::io::Result<bool> {
    let parent_stat = fstat(parent.raw()).map_err(io_error)?;
    let entry = fstatat(
        Some(parent.raw()),
        Path::new(name),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io_error)?;
    if !SFlag::from_bits_truncate(entry.st_mode).contains(SFlag::S_IFDIR)
        || entry.st_uid != parent_stat.st_uid
    {
        mark_cleanup_blocked(parent, target)?;
        return Err(identity_changed());
    }
    fchmodat(
        Some(parent.raw()),
        Path::new(name),
        Mode::from_bits_truncate(0o700),
        FchmodatFlags::NoFollowSymlink,
    )
    .map_err(io_error)?;
    let descriptor = open_directory_at(parent, name)?;
    let opened = fstat(descriptor.raw()).map_err(io_error)?;
    if opened.st_dev != entry.st_dev || opened.st_ino != entry.st_ino {
        mark_cleanup_blocked(parent, target)?;
        return Err(identity_changed());
    }
    let quarantine = Quarantine {
        name: name.to_os_string(),
        descriptor,
    };
    let artifact_entry = match fstatat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(entry) => entry,
        Err(Errno::ENOENT) => {
            if exact_name_is_absent(parent, target)? {
                remove_empty_quarantine(parent, &quarantine)?;
                return Ok(false);
            }
            fail_closed(parent, target, &quarantine)?;
            return Err(identity_changed());
        }
        Err(error) => return Err(io_error(error)),
    };
    if !SFlag::from_bits_truncate(artifact_entry.st_mode).contains(SFlag::S_IFREG)
        || artifact_entry.st_uid != opened.st_uid
    {
        fail_closed(parent, target, &quarantine)?;
        return Err(identity_changed());
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
    fchmod(quarantine.descriptor.raw(), Mode::empty()).map_err(io_error)?;
    if !exact_name_is_absent(parent, target)? {
        fail_closed(parent, target, &quarantine)?;
        return Err(identity_changed());
    }
    fchmod(quarantine.descriptor.raw(), Mode::from_bits_truncate(0o700)).map_err(io_error)?;
    unlinkat(
        Some(quarantine.descriptor.raw()),
        Path::new("artifact"),
        UnlinkatFlags::NoRemoveDir,
    )
    .map_err(io_error)?;
    remove_empty_quarantine(parent, &quarantine)?;
    Ok(true)
}

pub(super) fn remove_empty_quarantine(
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

pub(super) fn fail_closed(
    parent: &OwnedDescriptor,
    target: &std::ffi::OsStr,
    quarantine: &Quarantine,
) -> std::io::Result<()> {
    fchmod(quarantine.descriptor.raw(), Mode::empty()).map_err(io_error)?;
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
