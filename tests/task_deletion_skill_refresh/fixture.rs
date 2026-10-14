use super::support::Fixture;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
pub(super) fn target(path: &Path) {
    fs::write(
        path,
        r"#!/usr/bin/python3
import json, os, pathlib, sys
args = sys.argv[1:]
if args == ['--version']:
    print('brain 0.101.0'); sys.exit()
if args == ['--help']:
    print('Usage: brain --workspace -w'); sys.exit()
if args == ['skills', '--help']:
    print('Commands: sync'); sys.exit()
if len(args) == 5 and args[0] == '-w' and args[2:] == ['skills', 'sync', '--help']:
    print('Usage: brain skills sync'); sys.exit()
assert len(args) == 4 and args[0] == '-w' and args[2:] == ['skills', 'sync'], args
registry = json.loads((pathlib.Path(os.environ['XDG_CONFIG_HOME']) / 'brain/env.json').read_text())
assert list(registry['workspaces']) == [args[1]]
root = pathlib.Path(registry['workspaces'][args[1]]['root'])
assert root == pathlib.Path.cwd()
assert not registry['workspaces'][args[1]].get('env')
assert not registry.get('env')
assert not any(k.startswith('BRAIN_') for k in os.environ)
assert not (root / 'tasks/tasks.csv').exists()
body = 'legacy target body\n<!-- brain:ext todo:anchors -->\n'
ext = root / '.config/extensions/todo.md'
if ext.exists(): body = 'legacy target body\n' + ext.read_text().split('\n', 1)[1].strip()
out = root / '.agents/skills/todo'
out.mkdir(parents=True)
(out / 'SKILL.md').write_text(body)
(out / '.brain-rendered').write_text('0.101.0')
for frontend in ['.claude', '.codex', '.opencode']:
    link = root / frontend / 'skills/todo'
    link.parent.mkdir(parents=True)
    link.symlink_to(out)
",
    )
    .unwrap();
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
