use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::{OsStrExt as _, OsStringExt as _},
    path::Path,
};

use nix::{
    fcntl::{AtFlags, renameat},
    sys::stat::{SFlag, fstat, fstatat},
};

use super::Quarantine;
use crate::workspace::secure_remove::{
    OwnedDescriptor, exact_name_is_absent, identity_changed, io_error,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QuarantinePhase {
    Pending,
    Active,
}

pub(super) struct BoundQuarantine {
    pub(super) name: OsString,
    pub(super) phase: QuarantinePhase,
}

pub(super) fn pending_name(prefix: &str) -> OsString {
    OsString::from(format!("{prefix}{}-pending", uuid::Uuid::new_v4()))
}

pub(super) fn parse_bound_name(bytes: &[u8], prefix_len: usize) -> Option<BoundQuarantine> {
    let suffix = std::str::from_utf8(bytes.get(prefix_len..)?).ok()?;
    let (uuid, phase) = suffix.strip_suffix("-pending").map_or_else(
        || {
            suffix
                .strip_suffix("-active")
                .map_or((suffix, QuarantinePhase::Active), |uuid| {
                    (uuid, QuarantinePhase::Active)
                })
        },
        |uuid| (uuid, QuarantinePhase::Pending),
    );
    uuid::Uuid::parse_str(uuid)
        .ok()
        .filter(|value| value.get_version() == Some(uuid::Version::Random))?;
    Some(BoundQuarantine {
        name: OsString::from_vec(bytes.to_vec()),
        phase,
    })
}

pub(super) fn promote(
    parent: &OwnedDescriptor,
    quarantine: &mut Quarantine,
) -> std::io::Result<()> {
    if quarantine.phase == QuarantinePhase::Active {
        return Ok(());
    }
    let Some(stem) = quarantine.name.as_bytes().strip_suffix(b"-pending") else {
        return Err(identity_changed());
    };
    let mut active_bytes = stem.to_vec();
    active_bytes.extend_from_slice(b"-active");
    let active_name = OsString::from_vec(active_bytes);
    verify_held_name(parent, &quarantine.name, &quarantine.descriptor)?;
    if !exact_name_is_absent(parent, &active_name)? {
        return Err(identity_changed());
    }
    renameat(
        Some(parent.raw()),
        Path::new(&quarantine.name),
        Some(parent.raw()),
        Path::new(&active_name),
    )
    .map_err(io_error)?;
    quarantine.name = active_name;
    quarantine.phase = QuarantinePhase::Active;
    verify_held_name(parent, &quarantine.name, &quarantine.descriptor)
}

fn verify_held_name(
    parent: &OwnedDescriptor,
    name: &OsStr,
    descriptor: &OwnedDescriptor,
) -> std::io::Result<()> {
    let parent_stat = fstat(parent.raw()).map_err(io_error)?;
    let held = fstat(descriptor.raw()).map_err(io_error)?;
    let entry = fstatat(
        Some(parent.raw()),
        Path::new(name),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io_error)?;
    if entry.st_dev != held.st_dev
        || entry.st_ino != held.st_ino
        || !SFlag::from_bits_truncate(entry.st_mode).contains(SFlag::S_IFDIR)
        || entry.st_uid != parent_stat.st_uid
        || held.st_uid != parent_stat.st_uid
    {
        return Err(identity_changed());
    }
    Ok(())
}
