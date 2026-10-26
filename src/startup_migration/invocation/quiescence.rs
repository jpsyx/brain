//! Conservative evidence checks for processes predating invocation ownership.
use anyhow::{Result, ensure};
use std::{fs, os::unix::fs::MetadataExt as _, path::Path};

pub(super) fn check(home: &Path) -> Result<()> {
    let home = home.canonicalize()?;
    let store = crate::workspace::RegistryStore::real();
    let registry = match fs::symlink_metadata(store.path()) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        result => {
            result?;
            Some(crate::workspace::RegistryStore::load_readable(
                store.path(),
            )?)
        }
    };
    if let Some(registry) = registry {
        for record in registry.workspaces.values() {
            let relative = format!(".cache/brain/workspaces/{}", record.workspace_id);
            if let Some(bytes) = read(&home, Path::new(&relative), "tui.lock", 32)? {
                let pid = std::str::from_utf8(&bytes)?.trim().parse::<u32>()?;
                ensure!(
                    pid > 0 && !crate::server::lifecycle::pid_alive(pid),
                    "a Brain TUI is still active"
                );
            }
        }
    }
    if let Some(bytes) = read(
        &home,
        Path::new(".cache/brain/server"),
        "process.json",
        4096,
    )? {
        let record: crate::server::lifecycle::ProcessRecord = serde_json::from_slice(&bytes)?;
        ensure!(
            record.pid > 0 && !crate::server::lifecycle::pid_alive(record.pid),
            "a Brain server is still active"
        );
    }
    Ok(())
}

fn read(home: &Path, relative: &Path, name: &str, limit: usize) -> Result<Option<Vec<u8>>> {
    let path = home.join(relative);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        other => {
            other?;
        }
    }
    let directory = crate::workspace::open_verified_owned_directory_beneath(
        home,
        relative,
        fs::metadata(home)?.uid(),
    )?
    .ok_or_else(|| anyhow::anyhow!("untrusted Brain lifecycle evidence"))?;
    Ok(crate::workspace::read_small_owned_regular_file_in(
        &directory,
        Path::new(name),
        limit,
    )?)
}
