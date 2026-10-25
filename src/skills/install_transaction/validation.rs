//! Bounded journal and manifest parsing before deriving any recovery paths.
use super::{
    InstallIdentity, ManagedSkillManifest,
    journal::Journal,
    ownership,
    snapshot::{self, Snapshot},
};
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

pub(super) fn version(value: &str) -> bool {
    let (release, build) = value
        .split_once('+')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    if build.is_some_and(|identifiers| !version_identifiers(identifiers, false)) {
        return false;
    }
    let (core, prerelease) = release
        .split_once('-')
        .map_or((release, None), |(a, b)| (a, Some(b)));
    if prerelease.is_some_and(|identifiers| !version_identifiers(identifiers, true)) {
        return false;
    }
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}
fn version_identifiers(value: &str, reject_leading_zero: bool) -> bool {
    value.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !(reject_leading_zero
                && part.len() > 1
                && part.starts_with('0')
                && part.bytes().all(|byte| byte.is_ascii_digit()))
    })
}
pub(super) fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
pub(super) fn name(value: &str) -> Result<()> {
    snapshot::relative(value)?;
    ensure!(
        !value.contains('/') && value.len() <= 255 && !value.starts_with(".brain-skills-"),
        "invalid skill name"
    );
    Ok(())
}
pub(super) fn manifest(manifest: &ManagedSkillManifest, identity: &InstallIdentity) -> Result<()> {
    ensure!(
        matches!(manifest.schema_version, 1 | 2) && manifest.workspace_id == identity.workspace_id,
        "managed skills manifest identity/schema mismatch"
    );
    ensure!(
        version(&manifest.binary_version) && manifest.files.len() <= 100_000,
        "invalid managed skills manifest"
    );
    super::package::validate_metadata(manifest)?;
    for (path, sha256) in &manifest.files {
        snapshot::relative(path)?;
        let parts: Vec<_> = path.split('/').collect();
        ensure!(
            parts.len() >= 4 && parts[0] == ".agents" && parts[1] == "skills" && hash(sha256),
            "invalid managed skills path/digest"
        );
        name(parts[2])?;
    }
    for name in ownership::roots(manifest) {
        ensure!(
            manifest
                .files
                .contains_key(&format!(".agents/skills/{name}/SKILL.md"))
                && manifest
                    .files
                    .contains_key(&format!(".agents/skills/{name}/.brain-rendered")),
            "incomplete managed skill inventory"
        );
    }
    Ok(())
}
fn fingerprint(value: &Snapshot, budget: &mut usize) -> Result<()> {
    ensure!(*budget > 0, "oversized skills fingerprint");
    *budget -= 1;
    match value {
        Snapshot::File { sha256, .. } => ensure!(hash(sha256), "invalid fingerprint digest"),
        Snapshot::Directory { entries, .. } => {
            for (key, child) in entries {
                snapshot::relative(key)?;
                ensure!(
                    !key.contains('/') && *child != Snapshot::Absent,
                    "invalid fingerprint child"
                );
                fingerprint(child, budget)?;
            }
        }
        Snapshot::Link { target } => ensure!(
            target.len() <= 16_384 && !target.contains('\0'),
            "invalid symlink fingerprint"
        ),
        Snapshot::Absent => {}
    }
    Ok(())
}
pub(super) fn journal(journal: &Journal, identity: &InstallIdentity) -> Result<()> {
    ensure!(
        matches!(journal.schema_version, 1 | 2)
            && journal.root == identity.root
            && journal.workspace_id == identity.workspace_id,
        "skills recovery identity mismatch"
    );
    ensure!(
        uuid::Uuid::parse_str(&journal.transaction_id)?.to_string() == journal.transaction_id,
        "invalid transaction UUID"
    );
    ensure!(
        journal.next_sha256 == snapshot::digest(&journal.next)
            && journal.previous_sha256
                == journal
                    .previous
                    .as_ref()
                    .map(|bytes| snapshot::digest(bytes)),
        "skills journal manifest digest mismatch"
    );
    let next: ManagedSkillManifest = serde_json::from_slice(&journal.next)?;
    manifest(&next, identity)?;
    let old: Option<ManagedSkillManifest> = journal
        .previous
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()?;
    if let Some(old) = &old {
        manifest(old, identity)?;
    }
    super::package::validate_artifacts(journal, &next, old.as_ref())?;
    journal.verify_sources(&identity.root)?;
    journal.validate_archive(&identity.cache)?;
    let old_roots = old.as_ref().map_or_else(BTreeSet::new, ownership::roots);
    let roots: BTreeSet<_> = old_roots.union(&ownership::roots(&next)).cloned().collect();
    ensure!(journal.entries.len() <= 4096, "oversized skills journal");
    for (name, marker) in &journal.legacy {
        self::name(name)?;
        ensure!(
            version(marker.trim()) && old.is_none(),
            "invalid legacy adoption evidence"
        );
        let backup = identity
            .cache
            .join("legacy-backups")
            .join(&journal.transaction_id)
            .join(".agents/skills")
            .join(name);
        ensure!(
            matches!(snapshot::read(&backup.join(".brain-rendered"))?, Snapshot::File { sha256, .. } if sha256 == snapshot::digest(marker.as_bytes())),
            "legacy backup marker mismatch"
        );
    }
    for (name, evidence) in &journal.users {
        self::name(name)?;
        ensure!(
            !roots.contains(name) && matches!(evidence, Snapshot::File { .. }),
            "invalid user fan-out evidence"
        );
        let relative = format!(".agents/skills/{name}/SKILL.md");
        snapshot::check_parent(&identity.root, &relative)?;
        ensure!(
            snapshot::read(&identity.root.join(relative))? == *evidence,
            "user skill changed during interrupted fan-out"
        );
    }
    let mut paths = BTreeSet::new();
    let mut budget = 200_000;
    for entry in &journal.entries {
        snapshot::relative(&entry.live)?;
        if entry.kind.is_some() {
            ensure!(
                paths.insert(entry.live.clone()),
                "duplicate package artifact"
            );
            snapshot::check_parent(&identity.root, &entry.live)?;
            fingerprint(&entry.before, &mut budget)?;
            fingerprint(&entry.after, &mut budget)?;
            continue;
        }
        let parts: Vec<_> = entry.live.split('/').collect();
        ensure!(
            parts.len() == 3
                && [".agents", ".claude", ".codex", ".opencode"].contains(&parts[0])
                && parts[1] == "skills",
            "invalid recovery live path"
        );
        name(parts[2])?;
        ensure!(
            (roots.contains(parts[2]) || journal.users.contains_key(parts[2]))
                && paths.insert(entry.live.clone()),
            "unowned or duplicate recovery path"
        );
        snapshot::check_parent(&identity.root, &entry.live)?;
        fingerprint(&entry.before, &mut budget)?;
        fingerprint(&entry.after, &mut budget)?;
        if parts[0] == ".agents" {
            ensure!(roots.contains(parts[2]), "unowned canonical recovery tree");
            ensure!(
                matches!(entry.before, Snapshot::Absent | Snapshot::Directory { .. })
                    && matches!(entry.after, Snapshot::Absent | Snapshot::Directory { .. }),
                "invalid managed tree fingerprint"
            );
            ensure!(
                old_roots.contains(parts[2])
                    || entry.before == Snapshot::Absent
                    || journal.legacy.contains_key(parts[2]),
                "unowned recovery tree"
            );
            if journal.legacy.contains_key(parts[2]) {
                let backup = identity
                    .cache
                    .join("legacy-backups")
                    .join(&journal.transaction_id)
                    .join(&entry.live);
                ensure!(
                    snapshot::read(&backup)? == entry.before,
                    "legacy backup does not match recovery tree"
                );
            }
            for (path, sha256) in &next.files {
                if let Some(relative) = path.strip_prefix(&format!("{}/", entry.live)) {
                    ensure!(
                        matches!(at(&entry.after, relative), Some(Snapshot::File { sha256: actual, .. }) if actual == sha256),
                        "recovery output does not match manifest"
                    );
                }
            }
        } else {
            ensure!(
                entry.before == Snapshot::Absent
                    || ((old_roots.contains(parts[2]) || journal.legacy.contains_key(parts[2]))
                        && ownership::equivalent_link(
                            &identity.root,
                            &entry.live,
                            &entry.before,
                            parts[2]
                        )),
                "unowned recovery frontend"
            );
            ensure!(
                entry.after == Snapshot::Absent
                    || ownership::equivalent_link(
                        &identity.root,
                        &entry.live,
                        &entry.after,
                        parts[2]
                    ),
                "invalid frontend output"
            );
        }
    }
    for name in &roots {
        for parent in [".agents", ".claude", ".codex", ".opencode"] {
            ensure!(
                paths.contains(&format!("{parent}/skills/{name}")),
                "incomplete recovery inventory"
            );
        }
    }
    Ok(())
}
pub(super) fn at<'a>(tree: &'a Snapshot, path: &str) -> Option<&'a Snapshot> {
    let Snapshot::Directory { entries, .. } = tree else {
        return None;
    };
    path.split_once('/').map_or_else(
        || entries.get(path),
        |(first, rest)| at(entries.get(first)?, rest),
    )
}
