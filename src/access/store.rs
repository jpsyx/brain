use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use super::AccessMode;

static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn ensure_portable_access_mode(root: &Path, default: AccessMode) -> Result<()> {
    ensure_portable_access_mode_with_hook(root, default, || Ok(())).map(|_| ())
}

pub(crate) fn ensure_registry_access_modes(
    registry: &crate::workspace::MachineRegistry,
) -> Result<bool> {
    let mut changed = false;
    for (name, record) in &registry.workspaces {
        if !record.root.is_dir() {
            bail!(
                "workspace root {} is unavailable; cannot validate portable access mode",
                record.root.display()
            );
        }
        let default = if name == &registry.default_workspace {
            AccessMode::Unrestricted
        } else {
            AccessMode::WorkspaceOnly
        };
        changed |= ensure_portable_access_mode_with_hook(&record.root, default, || Ok(()))?;
    }
    Ok(changed)
}

pub(crate) fn load_portable_access_mode(root: &Path) -> Result<AccessMode> {
    let path = config_path(root);
    let config = load_config_map(&path)?;
    match config.get("access_mode") {
        Some(Value::String(value)) => AccessMode::parse(value).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid access_mode `{value}` in {} (expected unrestricted or workspace_only)",
                path.display()
            )
        }),
        Some(_) => bail!("access_mode in {} must be a string", path.display()),
        None => bail!("access_mode is missing from {}", path.display()),
    }
}

pub(crate) fn set_portable_access_mode(root: &Path, mode: AccessMode) -> Result<()> {
    let path = config_path(root);
    let mut config = load_config_map(&path)?;
    config.insert(
        "access_mode".to_owned(),
        Value::String(mode.as_config_value().to_owned()),
    );
    save_config_map(&path, &config, || Ok(()))
}

fn ensure_portable_access_mode_with_hook(
    root: &Path,
    default: AccessMode,
    before_replace: impl FnOnce() -> std::io::Result<()>,
) -> Result<bool> {
    let path = config_path(root);
    let mut config = load_config_map(&path)?;
    match config.get("access_mode") {
        Some(Value::String(value)) if AccessMode::parse(value).is_some() => return Ok(false),
        Some(Value::String(value)) => bail!(
            "invalid access_mode `{value}` in {} (expected unrestricted or workspace_only)",
            path.display()
        ),
        Some(_) => bail!("access_mode in {} must be a string", path.display()),
        None => {}
    }
    config.insert(
        "access_mode".to_owned(),
        Value::String(default.as_config_value().to_owned()),
    );
    save_config_map(&path, &config, before_replace)?;
    Ok(true)
}

fn config_path(root: &Path) -> PathBuf {
    root.join(".config/config.json")
}

fn load_config_map(path: &Path) -> Result<Map<String, Value>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("read {}", path.display()));
        }
    };
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {} as JSON", path.display()))?;
    match value {
        Value::Object(config) => Ok(config),
        _ => bail!("{} must contain a JSON object", path.display()),
    }
}

fn save_config_map(
    path: &Path,
    config: &Map<String, Value>,
    before_replace: impl FnOnce() -> std::io::Result<()>,
) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create config directory {}", parent.display()))?;
    let temporary = temporary_path(path);
    let result = write_and_replace(&temporary, path, config, before_replace);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let counter = TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.json");
    path.with_file_name(format!(
        ".{name}.tmp-{}-{nonce}-{counter}",
        std::process::id()
    ))
}

fn write_and_replace(
    temporary: &Path,
    path: &Path,
    config: &Map<String, Value>,
    before_replace: impl FnOnce() -> std::io::Result<()>,
) -> Result<()> {
    let body = serde_json::to_string_pretty(&Value::Object(config.clone()))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(temporary)
        .with_context(|| format!("create temporary config {}", temporary.display()))?;
    file.write_all(format!("{body}\n").as_bytes())
        .with_context(|| format!("write temporary config {}", temporary.display()))?;
    file.sync_all()
        .with_context(|| format!("sync temporary config {}", temporary.display()))?;
    drop(file);
    before_replace().with_context(|| format!("prepare to replace {}", path.display()))?;
    fs::rename(temporary, path).with_context(|| format!("replace config {}", path.display()))?;
    let directory = fs::File::open(path.parent().unwrap_or_else(|| Path::new(".")))
        .with_context(|| format!("open config directory for {}", path.display()))?;
    directory
        .sync_all()
        .with_context(|| format!("sync config directory for {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn malformed_or_non_object_config_is_preserved_byte_for_byte() {
        for malformed in [b"{broken".as_slice(), b"[]".as_slice()] {
            let temporary = tempfile::tempdir().unwrap();
            let path = temporary.path().join(".config/config.json");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, malformed).unwrap();

            let error = ensure_portable_access_mode(temporary.path(), AccessMode::WorkspaceOnly)
                .expect_err("invalid config must stop access-mode seeding");

            assert!(error.to_string().contains("config.json"));
            assert_eq!(std::fs::read(&path).unwrap(), malformed);
        }
    }

    #[test]
    fn interrupted_replace_keeps_live_config_and_retryable_temp_state() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join(".config");
        let path = directory.join("config.json");
        std::fs::create_dir_all(&directory).unwrap();
        let original = br#"{"unrelated":"kept"}"#;
        std::fs::write(&path, original).unwrap();

        let error = ensure_portable_access_mode_with_hook(
            temporary.path(),
            AccessMode::WorkspaceOnly,
            || Err(io::Error::other("injected interruption before replace")),
        )
        .expect_err("injected interruption must abort replacement");

        assert!(format!("{error:#}").contains("injected interruption"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);

        ensure_portable_access_mode(temporary.path(), AccessMode::WorkspaceOnly)
            .expect("retry access-mode seeding");
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["unrelated"], "kept");
        assert_eq!(saved["access_mode"], "workspace_only");
    }
}
