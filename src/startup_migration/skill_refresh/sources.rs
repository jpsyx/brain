//! Complete no-follow input snapshots for the audited renderer.
use crate::workspace::{
    InteractionMode, MachineRegistry, WorkspaceManifest, WorkspaceName, WorkspaceRecord,
    readiness_action_with_users,
};
use anyhow::{Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(PartialEq, Eq)]
pub(super) struct Sources {
    pub(super) files: BTreeMap<String, Vec<u8>>,
    pub(super) directories: Vec<PathBuf>,
    pub(super) access_mode: String,
}
impl Sources {
    pub(super) fn read(
        registry: &MachineRegistry,
        name: &WorkspaceName,
        record: &WorkspaceRecord,
        version: &str,
    ) -> Result<Self> {
        let root = &record.root;
        let manifest = read_file(&root.join(".config/workspace.json"))?;
        let validated = WorkspaceManifest::parse(&manifest, version)?;
        let users_path = root.join(".config/users.json");
        let users = optional_file(&users_path)?;
        let readiness = readiness_action_with_users(
            name,
            record,
            Ok(validated),
            crate::users::UsersStore::load_from(&users_path),
            InteractionMode::NonInteractive,
        )?;
        ensure!(
            matches!(readiness, crate::workspace::ReadinessAction::Ready(_)),
            "workspace membership requires setup"
        );
        let config: serde_json::Value = optional_file(&root.join(".config/config.json"))?
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()?
            .unwrap_or_else(|| serde_json::json!({}));
        ensure!(config.is_object(), "workspace config must be an object");
        let default = if name == &registry.default_workspace {
            "unrestricted"
        } else {
            "workspace_only"
        };
        let access_mode = match config.get("access_mode") {
            None => default,
            Some(value) => value
                .as_str()
                .filter(|v| matches!(*v, "unrestricted" | "workspace_only"))
                .ok_or_else(|| anyhow::anyhow!("invalid portable access mode"))?,
        }
        .to_owned();
        let mut files = BTreeMap::from([(".config/workspace.json".into(), manifest)]);
        if let Some(users) = users {
            files.insert(".config/users.json".into(), users);
        }
        let mut directories = Vec::new();
        for relative in [".config/extensions", ".config/plugins"] {
            walk(
                root,
                Path::new(relative),
                &mut files,
                &mut directories,
                true,
            )?;
        }
        Ok(Self {
            files,
            directories,
            access_mode,
        })
    }
    pub(super) fn copy_to(&self, root: &Path, customization: bool) -> Result<()> {
        for dir in &self.directories {
            if customization {
                fs::create_dir_all(root.join(dir))?;
            }
        }
        for (relative, bytes) in &self.files {
            if !customization
                && (relative.starts_with(".config/extensions/")
                    || relative.starts_with(".config/plugins/"))
            {
                continue;
            }
            let path = root.join(relative);
            fs::create_dir_all(path.parent().expect("relative source parent"))?;
            fs::write(path, bytes)?;
        }
        fs::write(
            root.join(".config/config.json"),
            serde_json::to_vec(
                &serde_json::json!({"access_mode":self.access_mode, "skills_auto_sync": false}),
            )?,
        )?;
        Ok(())
    }
}
pub(super) fn read_file(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        fs::symlink_metadata(path)?.is_file(),
        "expected regular file"
    );
    Ok(fs::read(path)?)
}
pub(super) fn optional_file(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        Ok(_) => read_file(path).map(Some),
    }
}
pub(super) fn walk(
    root: &Path,
    relative: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
    directories: &mut Vec<PathBuf>,
    optional: bool,
) -> Result<()> {
    let path = root.join(relative);
    let metadata = match fs::symlink_metadata(&path) {
        Err(e) if optional && e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        other => other?,
    };
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "source/output directory is not a real directory: {}",
        relative.display()
    );
    directories.push(relative.to_path_buf());
    let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("non-UTF-8 source path"))?;
        ensure!(
            !name.contains(['\\', '\n', '\r']) && name != "." && name != "..",
            "unsupported source path"
        );
        let relative = relative.join(name);
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(root, &relative, files, directories, false)?;
        } else {
            ensure!(
                kind.is_file(),
                "source/output is not a regular file: {}",
                relative.display()
            );
            let key = relative
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("invalid source path"))?
                .to_owned();
            ensure!(
                files.insert(key, fs::read(entry.path())?).is_none(),
                "duplicate source path"
            );
        }
    }
    Ok(())
}
