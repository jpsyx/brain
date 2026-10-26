use super::support::Fixture;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
pub(super) fn target(path: &Path) {
    fs::write(path, include_str!("target.py")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
pub(super) fn legacy_root(f: &mut Fixture, name: &str) -> std::path::PathBuf {
    let root = f.add(name);
    let path = root.join(".config/workspace.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["minimum_brain_version"] = "0.101.0".into();
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    root
}
pub(super) fn refresh(f: &Fixture, target: &Path, phase: &str) -> std::process::Output {
    let digest = format!("{:x}", Sha256::digest(fs::read(target).unwrap()));
    f.command()
        .args(["__refresh-workspace-skills", "--target-binary"])
        .arg(target)
        .args([
            "--target-version",
            "0.101.0",
            "--phase",
            phase,
            "--expected-target-sha256",
            &digest,
        ])
        .output()
        .unwrap()
}
