#[allow(dead_code, unused_imports)]
mod receiver_workspace_support;
#[path = "status_read_only/snapshot.rs"]
mod snapshot;

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sha2::{Digest as _, Sha256};

use receiver_workspace_support::DualWorkspaceReceiverFixture;
use snapshot::{snapshot, snapshot_entry};

#[test]
fn server_status_is_a_literal_read_only_process_probe() {
    let home = tempfile::tempdir().expect("temporary home");
    let before = snapshot(home.path());
    let before_logs = run_log_snapshot();

    let (pid, output) = run(home.path(), &["server", "status"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
    assert_eq!(
        pid_run_logs(pid, &run_log_snapshot()),
        pid_run_logs(pid, &before_logs),
        "status created or modified a PID run log"
    );
    let server = home.path().join(".cache/brain/server");
    assert!(!server.exists(), "status created server state");
}

#[test]
fn receiver_status_reads_four_fields_without_mutating_workspace_or_machine_state() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    let before = snapshot(home.path());
    let before_logs = run_log_snapshot();

    let (pid, output) = run(home.path(), &["-b", "brain", "receiver", "status"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
    assert_eq!(
        pid_run_logs(pid, &run_log_snapshot()),
        pid_run_logs(pid, &before_logs),
        "status created or modified a PID run log"
    );
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

#[test]
fn receiver_status_surfaces_a_live_process_control_failure() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    publish_live_process_record(home.path(), "57b162df-983a-45c3-ac7e-bad94eb27a99");

    let (_, output) = run(home.path(), &["-b", "brain", "receiver", "status"]);

    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(
        stderr.contains("connecting to the shared brain server"),
        "missing preserved control error:\n{stderr}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("TUI       not live"));
}

#[test]
fn receiver_status_rejects_generation_replacement_in_its_single_control_probe() {
    let home = tempfile::tempdir().expect("temporary home");
    seed_ready_workspace(home.path());
    let paths = brain::server::lifecycle::ServerPaths::from_home(home.path());
    std::fs::create_dir_all(paths.directory()).expect("server state directory");
    let listener = UnixListener::bind(paths.control_socket()).expect("control listener");
    publish_live_process_record(home.path(), "57b162df-983a-45c3-ac7e-bad94eb27a99");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept status probe");
        let request =
            brain::server::control::codec::read::<brain::server::control::ControlRequest>(
                &mut stream,
            )
            .expect("read status probe");
        assert!(matches!(
            request,
            brain::server::control::ControlRequest::WorkspaceStatus { .. }
        ));
        brain::server::control::codec::write(
            &mut stream,
            &brain::server::control::ControlResponse::StaleGeneration,
        )
        .expect("write stale generation");
    });

    let (_, output) = run(home.path(), &["-b", "brain", "receiver", "status"]);

    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(stderr.contains("generation changed"), "{stderr}");
    server.join().expect("status probe server");
}

#[test]
fn concurrent_status_commands_leave_an_active_generation_exactly_unchanged() {
    let mut fixture = DualWorkspaceReceiverFixture::start();
    let before_filesystem = snapshot(fixture.home());
    let before_server = fixture.server_snapshot();
    let control_socket = fixture.home().join(".cache/brain/server/control.sock");
    let before_control_socket = snapshot_entry(&control_socket);
    let before_logs = run_log_snapshot();

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));
    let workers = (0..8)
        .map(|index| {
            let home = fixture.home().to_path_buf();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                if index % 2 == 0 {
                    run(&home, &["server", "status"])
                } else {
                    run(&home, &["-b", "personal", "receiver", "status"])
                }
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let mut pids = Vec::with_capacity(workers.len());
    for worker in workers {
        let (pid, output) = worker.join().expect("status worker");
        pids.push(pid);
        assert!(output.status.success(), "{output:?}");
    }
    let after_logs = run_log_snapshot();
    for pid in pids {
        assert!(
            pid_run_logs_unchanged(pid, &before_logs, &after_logs),
            "active status created or modified a PID run log for {pid}"
        );
    }

    let after_server = fixture.server_snapshot();
    assert_eq!(after_server, before_server);
    assert_eq!(snapshot(fixture.home()), before_filesystem);
    assert_eq!(snapshot_entry(&control_socket), before_control_socket);
    assert!(fixture.server_is_running());
    fixture.shutdown();
}

#[test]
fn receiver_status_is_read_only_through_symlinked_config_and_workspace_paths() {
    let home = tempfile::tempdir().expect("temporary home");
    let external = tempfile::tempdir().expect("external status state");
    seed_ready_workspace(home.path());
    let external_config = external.path().join("config");
    let external_brain = external.path().join("brain");
    std::fs::rename(home.path().join(".config"), &external_config).expect("move machine config");
    std::fs::rename(home.path().join("brain"), &external_brain).expect("move workspace");
    std::os::unix::fs::symlink(&external_config, home.path().join(".config"))
        .expect("link machine config");
    std::os::unix::fs::symlink(&external_brain, home.path().join("brain")).expect("link workspace");
    std::os::unix::fs::symlink(home.path(), external_brain.join("cycle"))
        .expect("link snapshot cycle");
    let before = snapshot(home.path());

    let (_, output) = run(home.path(), &["-b", "brain", "receiver", "status"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(home.path()), before);
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunLogEntry {
    device: u64,
    inode: u64,
    mode: u32,
    hard_links: u64,
    uid: u32,
    gid: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
    bytes: Vec<u8>,
    sha256: [u8; 32],
}

fn run_log_snapshot() -> BTreeMap<PathBuf, RunLogEntry> {
    std::fs::read_dir("/tmp")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy();
            is_brain_run_log_name(&name).then(|| {
                let metadata = std::fs::metadata(&path).expect("run log metadata");
                let bytes = std::fs::read(&path).expect("run log bytes");
                let sha256 = Sha256::digest(&bytes).into();
                (
                    path,
                    RunLogEntry {
                        device: metadata.dev(),
                        inode: metadata.ino(),
                        mode: metadata.mode(),
                        hard_links: metadata.nlink(),
                        uid: metadata.uid(),
                        gid: metadata.gid(),
                        size: metadata.len(),
                        modified_seconds: metadata.mtime(),
                        modified_nanoseconds: metadata.mtime_nsec(),
                        changed_seconds: metadata.ctime(),
                        changed_nanoseconds: metadata.ctime_nsec(),
                        bytes,
                        sha256,
                    },
                )
            })
        })
        .collect()
}

fn is_brain_run_log_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".log") else {
        return false;
    };
    let Some((timestamp, pid)) = stem.rsplit_once('-') else {
        return false;
    };
    timestamp.contains('T') && pid.chars().all(|character| character.is_ascii_digit())
}

fn pid_run_logs(
    pid: u32,
    snapshot: &BTreeMap<PathBuf, RunLogEntry>,
) -> BTreeMap<PathBuf, RunLogEntry> {
    let suffix = format!("-{pid}.log");
    snapshot
        .iter()
        .filter(|(path, _)| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(&suffix))
        })
        .map(|(path, entry)| (path.clone(), entry.clone()))
        .collect()
}

fn pid_run_logs_unchanged(
    pid: u32,
    before: &BTreeMap<PathBuf, RunLogEntry>,
    after: &BTreeMap<PathBuf, RunLogEntry>,
) -> bool {
    pid_run_logs(pid, before) == pid_run_logs(pid, after)
}

#[test]
fn pid_log_observer_detects_same_size_reused_pid_log_mutation() {
    let pid = std::process::id();
    let suffix = format!("-{pid}.log");
    let log = tempfile::Builder::new()
        .prefix("2026-08-05T00:00:00.000000000-04:00-")
        .suffix(&suffix)
        .tempfile_in("/tmp")
        .expect("unique reused-PID run log");
    std::fs::write(log.path(), b"before").expect("seed reused-PID run log");
    let before = run_log_snapshot();

    std::fs::write(log.path(), b"after!").expect("mutate reused-PID run log");
    let after = run_log_snapshot();

    assert!(!pid_run_logs_unchanged(pid, &before, &after));
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

fn publish_live_process_record(home: &Path, generation: &str) {
    let paths = brain::server::lifecycle::ServerPaths::from_home(home);
    std::fs::create_dir_all(paths.directory()).expect("server state directory");
    std::fs::write(
        paths.process_record(),
        serde_json::to_vec(&serde_json::json!({
            "pid": std::process::id(),
            "port": 8787,
            "generation": generation,
            "started_at": "2026-08-05T12:00:00Z"
        }))
        .expect("process record JSON"),
    )
    .expect("process record");
}
