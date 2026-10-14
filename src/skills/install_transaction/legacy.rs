//! Conservative first adoption: retain complete verified copies in the cache.
use super::{
    InstallIdentity, ownership,
    snapshot::{self, Snapshot},
    validation,
};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, fs};

pub(super) fn discover(identity: &InstallIdentity) -> Result<BTreeMap<String, String>> {
    snapshot::check_parent(&identity.root, ".agents/skills/probe")?;
    let entries = match fs::read_dir(identity.root.join(".agents/skills")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error.into()),
    };
    let mut legacy = BTreeMap::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("non-UTF-8 skill name"))?;
        if name.starts_with(".brain-skills-") {
            continue;
        }
        validation::name(&name)?;
        let marker = entry.path().join(".brain-rendered");
        if !matches!(snapshot::read(&marker)?, Snapshot::File { .. }) {
            continue;
        }
        let bytes = fs::read_to_string(marker)?;
        if validation::version(bytes.trim()) {
            legacy.insert(name, bytes);
        }
    }
    Ok(legacy)
}

pub(super) fn backup(
    identity: &InstallIdentity,
    transaction_id: &str,
    legacy: &BTreeMap<String, String>,
) -> Result<()> {
    if legacy.is_empty() {
        return Ok(());
    }
    let relative = format!("legacy-backups/{transaction_id}/inventory.json");
    snapshot::ensure_parent(&identity.cache, &relative)?;
    let dest = identity.cache.join("legacy-backups").join(transaction_id);
    let mut inventory = BTreeMap::new();
    for name in legacy.keys() {
        let tree = format!(".agents/skills/{name}");
        let before = snapshot::read(&identity.root.join(&tree))?;
        snapshot::ensure_parent(&dest, &tree)?;
        snapshot::copy(&identity.root.join(&tree), &dest.join(&tree), &before)?;
        inventory.insert(tree, before);
        for frontend in [".claude", ".codex", ".opencode"] {
            let relative = format!("{frontend}/skills/{name}");
            snapshot::check_parent(&identity.root, &relative)?;
            let before = snapshot::read(&identity.root.join(&relative))?;
            if ownership::equivalent_link(&identity.root, &relative, &before, name) {
                snapshot::ensure_parent(&dest, &relative)?;
                snapshot::copy(
                    &identity.root.join(&relative),
                    &dest.join(&relative),
                    &before,
                )?;
                inventory.insert(relative, before);
            }
        }
    }
    snapshot::atomic_write(
        &dest.join("inventory.json"),
        &serde_json::to_vec(&inventory)?,
    )?;
    for (relative, expected) in inventory {
        ensure!(
            snapshot::read(&dest.join(relative))? == expected,
            "legacy backup verification failed"
        );
    }
    snapshot::sync_dir(&dest)?;
    snapshot::sync_dir(dest.parent().expect("validated artifact parent"))?;
    snapshot::sync_dir(&identity.cache)
}
