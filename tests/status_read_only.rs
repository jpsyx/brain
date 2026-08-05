use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

#[test]
fn server_status_is_a_literal_read_only_process_probe() {
    let home = tempfile::tempdir().expect("temporary home");
    let before = snapshot(home.path());

    let (pid, output) = run(home.path(), &["server", "status"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
    let logs = pid_run_logs(pid).collect::<Vec<_>>();
    assert!(logs.is_empty(), "status wrote run logs: {logs:?}");
    let server = home.path().join(".cache/brain/server");
    assert!(!server.exists(), "status created server state");
}

#[test]
fn receiver_status_reads_four_fields_without_mutating_workspace_or_machine_state() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    let before = snapshot(home.path());

    let (pid, output) = run(home.path(), &["-b", "brain", "receiver", "status"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
    let logs = pid_run_logs(pid).collect::<Vec<_>>();
    assert!(logs.is_empty(), "status wrote run logs: {logs:?}");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    for line in [
        "Receiver  enabled",
        "TUI       not live",
        "Server    not running",
        "Accepting no",
    ] {
        assert!(stdout.contains(line), "missing `{line}` in:\n{stdout}");
    }
    assert!(!home.path().join(".cache/brain/server").exists());
}

fn run(home: &Path, arguments: &[&str]) -> (u32, Output) {
    let child = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(arguments)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn brain status");
    let pid = child.id();
    let output = child.wait_with_output().expect("wait for brain status");
    (pid, output)
}

fn pid_run_logs(pid: u32) -> impl Iterator<Item = PathBuf> {
    let suffix = format!("-{pid}.log");
    std::fs::read_dir("/tmp")
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(move |path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(&suffix))
        })
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Entry> {
    let mut entries = BTreeMap::new();
    snapshot_directory(root, root, &mut entries);
    entries
}

fn snapshot_directory(root: &Path, directory: &Path, entries: &mut BTreeMap<PathBuf, Entry>) {
    let mut children = std::fs::read_dir(directory)
        .expect("read snapshot directory")
        .map(|entry| entry.expect("read snapshot entry"))
        .collect::<Vec<_>>();
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        let path = child.path();
        let relative = path
            .strip_prefix(root)
            .expect("snapshot prefix")
            .to_path_buf();
        let file_type = child.file_type().expect("snapshot file type");
        if file_type.is_dir() {
            entries.insert(relative, Entry::Directory);
            snapshot_directory(root, &path, entries);
        } else if file_type.is_symlink() {
            entries.insert(
                relative,
                Entry::Symlink(std::fs::read_link(path).expect("snapshot symlink")),
            );
        } else {
            entries.insert(
                relative,
                Entry::File(std::fs::read(path).expect("snapshot file")),
            );
        }
    }
}

fn seed_ready_workspace(home: &Path) {
    let root = home.join("brain");
    std::fs::create_dir_all(root.join(".config")).expect("workspace config directory");
    std::fs::create_dir_all(home.join(".config/brain")).expect("machine config directory");
    std::fs::write(
        root.join(".config/workspace.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "workspace_id": "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
            "receiver_ingress_id": "91a0cfc2-7427-49d5-a2f1-258f985cd7e5",
            "minimum_brain_version": env!("CARGO_PKG_VERSION")
        }))
        .expect("manifest JSON"),
    )
    .expect("workspace manifest");
    std::fs::write(
        root.join(".config/users.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "users": [{
                "id": "pablo",
                "name": "Pablo",
                "phones": [],
                "emails": [],
                "response_email": null
            }]
        }))
        .expect("users JSON"),
    )
    .expect("portable users");
    std::fs::write(
        home.join(".config/brain/env.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 2,
            "default_workspace": "brain",
            "workspaces": {
                "brain": {
                    "workspace_id": "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
                    "root": root,
                    "aliases": [],
                    "local_user_id": "pablo",
                    "receiver_enabled": true,
                    "env": {}
                }
            }
        }))
        .expect("registry JSON"),
    )
    .expect("machine registry");
}
