//! Read-only compatibility observation before taking the portable writer boundary.
use super::{InstallIdentity, InstallVersion, WorkspaceStamp, lock};
use crate::skills::install_transaction::{Currentness, ManagedSkillManifest};
use anyhow::Result;

pub(super) fn complete(manifest: Option<&ManagedSkillManifest>) -> bool {
    manifest
        .and_then(|manifest| manifest.package.as_ref())
        .is_some_and(|package| {
            package.instructions.is_some() && package.extension_snapshot.is_some()
        })
}

pub(super) fn matches(identity: &InstallIdentity, stamp: &WorkspaceStamp) -> Result<bool> {
    if identity.sync_active()? {
        return Ok(false);
    }
    let locked = lock::acquire(identity)?;
    if identity.sync_active()? || identity.receive_phase()?.is_some() {
        return Ok(false);
    }
    let before = stamp.peek()?;
    if !complete(locked.manifest()?.as_ref()) {
        return Ok(false);
    }
    let Ok(Currentness::Current(receipt)) = locked.inspect(&InstallVersion::current()) else {
        return Ok(false);
    };
    Ok(before.as_ref() == Some(&receipt)
        && stamp.peek()?.as_ref() == Some(&receipt)
        && identity.receive_phase()?.is_none()
        && !identity.sync_active()?)
}
