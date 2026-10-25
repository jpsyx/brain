//! Read-only ownership planning; only recorded generated files may be replaced.
use super::{
    InstallIdentity, InstallVersion, ManagedSkillManifest, RenderedInstall,
    journal::Entry,
    snapshot::{self, Snapshot},
};
use crate::skills::diagnostic::{Category, Diagnostic};
use anyhow::{Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

pub(super) struct Plan {
    pub entries: Vec<Entry>,
    pub files: BTreeMap<String, Vec<u8>>,
    pub manifest: ManagedSkillManifest,
    pub legacy: BTreeMap<String, String>,
    pub users: BTreeMap<String, Snapshot>,
    pub artifacts: BTreeMap<String, super::package::FileChange>,
    pub retained_package: Option<super::package::PackageMetadata>,
}

pub(super) fn roots(manifest: &ManagedSkillManifest) -> BTreeSet<String> {
    manifest
        .files
        .keys()
        .filter_map(|path| path.split('/').nth(2).map(str::to_owned))
        .collect()
}

pub(super) fn check_owned(
    identity: &InstallIdentity,
    previous: &ManagedSkillManifest,
) -> Result<Vec<String>> {
    let mut missing = Vec::new();
    for (path, expected) in &previous.files {
        snapshot::check_parent(&identity.root, path)?;
        match snapshot::read(&identity.root.join(path))? {
            Snapshot::Absent => missing.push(path.clone()),
            Snapshot::File { sha256, .. } => {
                ensure!(
                    &sha256 == expected,
                    Diagnostic::new(
                        Category::ManagedContentModified,
                        "verify_ownership",
                        Some(path)
                    )
                );
            }
            _ => {
                return Err(Diagnostic::new(
                    Category::OwnershipConflict,
                    "verify_ownership",
                    Some(path),
                )
                .into());
            }
        }
    }
    Ok(missing)
}

pub(super) fn plan(
    identity: &InstallIdentity,
    previous: Option<&ManagedSkillManifest>,
    mut rendered: RenderedInstall,
    version: &InstallVersion,
) -> Result<Plan> {
    ensure!(
        super::validation::version(&version.binary_version),
        "invalid install binary version"
    );
    let owned = previous.map_or_else(BTreeSet::new, roots);
    let legacy = if previous.is_none() {
        super::legacy::discover(identity)?
    } else {
        BTreeMap::new()
    };
    if let Some(previous) = previous {
        check_owned(identity, previous)?;
    }
    let mut manifest = ManagedSkillManifest {
        schema_version: 1,
        workspace_id: identity.workspace_id.clone(),
        binary_version: version.binary_version.clone(),
        compatibility_revision: version.compatibility_revision,
        files: BTreeMap::new(),
        package: previous.and_then(|manifest| manifest.package.clone()),
    };
    if manifest.package.is_some() {
        manifest.schema_version = 2;
    }
    let mut files = BTreeMap::new();
    for (name, content) in &mut rendered {
        super::validation::name(name)?;
        ensure!(
            content.contains_key("SKILL.md"),
            "missing SKILL.md for {name}"
        );
        content.insert(
            ".brain-rendered".into(),
            format!("{}\n", version.binary_version).into_bytes(),
        );
        for (relative, bytes) in content {
            snapshot::relative(relative)?;
            let path = format!(".agents/skills/{name}/{relative}");
            manifest.files.insert(path.clone(), snapshot::digest(bytes));
            files.insert(path, bytes.clone());
        }
    }
    let names: BTreeSet<_> = owned.iter().chain(rendered.keys()).cloned().collect();
    let mut entries = Vec::new();
    for name in names {
        let live = format!(".agents/skills/{name}");
        snapshot::check_parent(&identity.root, &live)?;
        let mut entry = Entry::new(live.clone(), &identity.root)?;
        ensure!(
            entry.before == Snapshot::Absent || matches!(entry.before, Snapshot::Directory { .. }),
            "skill directory changed type: {name}"
        );
        ensure!(
            entry.before == Snapshot::Absent || owned.contains(&name) || legacy.contains_key(&name),
            "unmarked skill collision: {name}"
        );
        let mut after = entry.before.clone();
        if let Some(previous) = previous {
            for path in previous
                .files
                .keys()
                .filter_map(|path| path.strip_prefix(&format!("{live}/")))
            {
                remove_owned(&mut after, path);
            }
        }
        if legacy.contains_key(&name) {
            if let Some(content) = rendered.get(&name) {
                for path in content.keys() {
                    ensure!(
                        super::validation::at(&after, path)
                            .is_none_or(|value| matches!(value, Snapshot::File { .. })),
                        "legacy generated path collides with non-file: {path}"
                    );
                    remove_owned(&mut after, path);
                }
            }
        }
        if let Some(content) = rendered.get(&name) {
            if after == Snapshot::Absent {
                after = empty_directory();
            }
            for (relative, bytes) in content {
                insert(
                    &mut after,
                    relative,
                    Snapshot::File {
                        sha256: snapshot::digest(bytes),
                        mode: 0o100_644,
                    },
                )?;
            }
        } else if matches!(&after, Snapshot::Directory { entries, .. } if entries.is_empty()) {
            after = Snapshot::Absent;
        }
        entry.after = after;
        entries.push(entry);
        for frontend in [".claude", ".codex", ".opencode"] {
            let live = format!("{frontend}/skills/{name}");
            snapshot::check_parent(&identity.root, &live)?;
            let mut entry = Entry::new(live, &identity.root)?;
            if entry.before != Snapshot::Absent {
                ensure!(
                    (owned.contains(&name) || legacy.contains_key(&name))
                        && equivalent_link(&identity.root, &entry.live, &entry.before, &name),
                    "protected frontend collision: {}",
                    entry.live
                );
            }
            entry.after = if rendered.contains_key(&name) {
                Snapshot::Link {
                    target: format!("../../.agents/skills/{name}"),
                }
            } else {
                Snapshot::Absent
            };
            entries.push(entry);
        }
    }
    let mut users = BTreeMap::new();
    for name in super::super::plugin::discover_names(&identity.root.join(".agents/skills")) {
        if rendered.contains_key(&name) || owned.contains(&name) {
            continue;
        }
        super::validation::name(&name)?;
        let skill = format!(".agents/skills/{name}/SKILL.md");
        let evidence = snapshot::read(&identity.root.join(skill))?;
        users.insert(name.clone(), evidence);
        for frontend in [".claude", ".codex", ".opencode"] {
            let live = format!("{frontend}/skills/{name}");
            snapshot::check_parent(&identity.root, &live)?;
            let mut entry = Entry::new(live, &identity.root)?;
            if entry.before == Snapshot::Absent {
                entry.after = Snapshot::Link {
                    target: format!("../../.agents/skills/{name}"),
                };
                entries.push(entry);
            }
        }
    }
    Ok(Plan {
        entries,
        files,
        manifest,
        legacy,
        users,
        artifacts: BTreeMap::new(),
        retained_package: None,
    })
}

pub(super) fn equivalent_link(root: &Path, live: &str, observed: &Snapshot, name: &str) -> bool {
    let Snapshot::Link { target } = observed else {
        return false;
    };
    let link = root.join(live);
    let resolved = link
        .parent()
        .expect("validated artifact parent")
        .join(target);
    let resolved = normalize(&resolved);
    let suffix = Path::new(".agents/skills").join(name);
    if resolved == root.join(&suffix) {
        return true;
    }
    // Normalize only the workspace-root alias (for example /var on macOS),
    // never the potentially dangling skill target or its inner ancestors.
    resolved.ends_with(&suffix)
        && resolved
            .ancestors()
            .nth(3)
            .and_then(|prefix| prefix.canonicalize().ok())
            .is_some_and(|prefix| prefix == root)
}
fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other),
        }
    }
    result
}
fn empty_directory() -> Snapshot {
    Snapshot::Directory {
        mode: 0o40755,
        entries: BTreeMap::new(),
    }
}
fn remove_owned(tree: &mut Snapshot, path: &str) {
    let Snapshot::Directory { entries, .. } = tree else {
        return;
    };
    if let Some((first, rest)) = path.split_once('/') {
        if let Some(child) = entries.get_mut(first) {
            remove_owned(child, rest);
            if matches!(child, Snapshot::Directory { entries, .. } if entries.is_empty()) {
                entries.remove(first);
            }
        }
    } else {
        entries.remove(path);
    }
}
fn insert(tree: &mut Snapshot, path: &str, file: Snapshot) -> Result<()> {
    let Snapshot::Directory { entries, .. } = tree else {
        anyhow::bail!("generated file collides with non-directory ancestor: {path}");
    };
    if let Some((first, rest)) = path.split_once('/') {
        insert(
            entries.entry(first.into()).or_insert_with(empty_directory),
            rest,
            file,
        )
    } else {
        ensure!(
            !entries.contains_key(path),
            "generated file collides with unowned extra: {path}"
        );
        entries.insert(path.into(), file);
        Ok(())
    }
}
