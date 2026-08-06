use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use brain::workspace::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryStore, WorkspaceId, WorkspaceManifest,
    WorkspaceName, WorkspaceRecord, migrate_legacy,
};
use serde_json::json;

fn run_migration(home: &std::path::Path, config_home: &std::path::Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "workspace",
            "repair",
            "--manifest",
            "--local-user-id",
            "migration-user",
        ])
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config_home)
        .env("NO_COLOR", "1")
        .output()
        .expect("run brain migration");
    assert!(
        output.status.success(),
        "migration command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture_dirs() -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().expect("home fixture");
    let config_home = tempfile::tempdir().expect("config fixture");
    let config_dir = config_home.path().join("brain");
    fs::create_dir_all(&config_dir).expect("create machine config");
    (home, config_home, config_dir)
}

fn backup_files(config_dir: &Path) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(config_dir)
        .expect("read machine config")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("env.json.legacy-backup"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}
