use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use brain::tasks::identity::{CsvKind, legacy_task_uuid};
use brain::workspace::{WorkspaceId, WorkspaceManifest};

use super::rclone_available;

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

#[test]
fn second_configured_legacy_machine_joins_current_remote_through_real_coordinator_and_rclone() {
    if !rclone_available() {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let fixture = Fixture::new();

    Fixture::assert_success(
        &fixture.run_b(&["sync", "repair", "-b", "family"]),
        "establish second-machine legacy baseline",
    );
    Fixture::assert_success(
        &fixture.run_a(&["sync", "repair", "-b", "family"]),
        "establish first-machine legacy baseline",
    );
    fixture.write_a_change();
    Fixture::assert_success(&fixture.migrate_a(), "migrate first machine");
    let first_uuid = task_rows(&fixture.remote)["T1"]["task_uuid"].clone();
    let ordinary_repair = fixture.run_b(&["sync", "repair", "-b", "family"]);
    assert!(!ordinary_repair.status.success());
    assert!(
        String::from_utf8_lossy(&ordinary_repair.stderr)
            .contains("remote task schema is Current, but local task schema is Legacy"),
        "{}",
        String::from_utf8_lossy(&ordinary_repair.stderr)
    );

    fixture.write_b_changes();
    Fixture::assert_success(&fixture.migrate_b(), "migrate second machine");

    let joined = task_rows(&fixture.root_b);
    assert_eq!(joined["T1"]["task_uuid"], first_uuid);
    assert_eq!(joined["T1"]["notes"], "first-machine-note");
    assert_eq!(joined["T1"]["status"], "waiting");
    assert_eq!(
        joined["T2"]["task_uuid"],
        legacy_task_uuid(workspace_id(), CsvKind::Tasks, "T2").to_string()
    );
    assert_eq!(joined["T2"]["task_name"], "Second-machine only");
    assert!(!fixture.migration_journal_b().exists());

    fixture.repair_a_until_clean();
    fixture.repair_b_until_clean();
    fixture.repair_a_until_clean();
    for relative in ["tasks/tasks.csv", "tasks/habits.csv", "tasks/SCHEMA.json"] {
        assert_eq!(
            std::fs::read(fixture.root_a.join(relative)).unwrap(),
            std::fs::read(fixture.root_b.join(relative)).unwrap(),
            "machines did not converge for {relative}"
        );
        assert_eq!(
            std::fs::read(fixture.root_b.join(relative)).unwrap(),
            std::fs::read(fixture.remote.join(relative)).unwrap(),
            "remote did not converge for {relative}"
        );
    }
}

struct Fixture {
    _temporary: tempfile::TempDir,
    remote: PathBuf,
    root_a: PathBuf,
    root_b: PathBuf,
    home_a: PathBuf,
    home_b: PathBuf,
    config_a: PathBuf,
    config_b: PathBuf,
    bin: PathBuf,
    real_rclone: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let remote = temporary.path().join("remote");
        let root_a = temporary.path().join("machine-a/workspace");
        let root_b = temporary.path().join("machine-b/workspace");
        let home_a = temporary.path().join("machine-a/home");
        let home_b = temporary.path().join("machine-b/home");
        let config_a = temporary.path().join("machine-a/config");
        let config_b = temporary.path().join("machine-b/config");
        let bin = temporary.path().join("bin");
        for directory in [&remote, &root_a, &root_b, &home_a, &home_b, &bin] {
            std::fs::create_dir_all(directory).unwrap();
        }
        write_legacy_workspace(&root_a);
        write_legacy_workspace(&root_b);
        write_remote_legacy(&remote, &root_a);
        write_registry(&config_a, &root_a);
        write_registry(&config_b, &root_b);
        let real_rclone = find_rclone();
        write_rclone_shim(&bin.join("rclone"));
        Self {
            _temporary: temporary,
            remote,
            root_a,
            root_b,
            home_a,
            home_b,
            config_a,
            config_b,
            bin,
            real_rclone,
        }
    }

    fn migrate_a(&self) -> Output {
        self.run_a(&[
            "workspace",
            "migrate",
            "-b",
            "family",
            "--acknowledge-all-machines-updated",
        ])
    }

    fn migrate_b(&self) -> Output {
        self.run_b(&[
            "workspace",
            "migrate",
            "-b",
            "family",
            "--acknowledge-all-machines-updated",
        ])
    }

    fn run_a(&self, args: &[&str]) -> Output {
        self.run(args, &self.home_a, &self.config_a)
    }

    fn run_b(&self, args: &[&str]) -> Output {
        self.run(args, &self.home_b, &self.config_b)
    }

    fn run(&self, args: &[&str], home: &Path, config: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", config)
            .env("NO_COLOR", "1")
            .env("REMOTE_ROOT", &self.remote)
            .env("REAL_RCLONE", &self.real_rclone)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.bin.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .output()
            .unwrap()
    }

    fn assert_success(output: &Output, phase: &str) {
        assert!(
            output.status.success(),
            "{phase} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn assert_sync_complete(output: &Output, phase: &str) {
        Self::assert_success(output, phase);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("sync complete."),
            "{phase} did not complete cleanly\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repair_a_until_clean(&self) {
        let mut last = None;
        for _ in 0..3 {
            let output = self.run_a(&["sync", "repair", "-b", "family"]);
            if String::from_utf8_lossy(&output.stdout).contains("sync complete.") {
                return;
            }
            last = Some(output);
        }
        Self::assert_sync_complete(last.as_ref().unwrap(), "repair first-machine baseline");
    }

    fn repair_b_until_clean(&self) {
        let mut last = None;
        for _ in 0..3 {
            let output = self.run_b(&["sync", "repair", "-b", "family"]);
            if String::from_utf8_lossy(&output.stdout).contains("sync complete.") {
                return;
            }
            last = Some(output);
        }
        Self::assert_sync_complete(last.as_ref().unwrap(), "repair second-machine baseline");
    }

    fn write_a_change(&self) {
        write_tasks(
            &self.root_a,
            "task_id,task_name,status,notes,assigned_to\n\
             T1,Shared,not_started,first-machine-note,pablo\n",
        );
    }

    fn write_b_changes(&self) {
        write_tasks(
            &self.root_b,
            "task_id,task_name,status,notes,assigned_to\n\
             T1,Shared,waiting,,pablo\n\
             T2,Second-machine only,not_started,,pablo\n",
        );
    }

    fn migration_journal_b(&self) -> PathBuf {
        self.home_b
            .join(".cache/brain/workspaces")
            .join(WORKSPACE_ID)
            .join("migrations/multi-workspace-v1.json")
    }
}

fn write_legacy_workspace(root: &Path) {
    WorkspaceManifest::new(workspace_id())
        .write_new(root)
        .unwrap();
    std::fs::write(
        root.join(".config/users.json"),
        b"{\"schema_version\":1,\"users\":[{\"id\":\"pablo\",\"name\":\"Pablo\",\"phones\":[],\"emails\":[],\"response_email\":null}]}\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".config/config.json"),
        b"{\"access_mode\":\"unrestricted\",\"enable_triage_habits\":false}\n",
    )
    .unwrap();
    write_tasks(
        root,
        "task_id,task_name,status,notes,assigned_to\n\
         T1,Shared,not_started,,pablo\n",
    );
    std::fs::write(
        root.join("tasks/habits.csv"),
        b"task_id,task_name,status,notes,assigned_to\nH1,Walk,not_started,,pablo\n",
    )
    .unwrap();
    std::fs::write(root.join("tasks/SCHEMA.json"), b"{}\n").unwrap();
    std::fs::write(
        root.join("RCLONE_TEST"),
        b"brain sync check-access marker\n",
    )
    .unwrap();
    std::fs::write(root.join("stable.md"), b"unchanged portable file\n").unwrap();
}

fn write_remote_legacy(remote: &Path, source: &Path) {
    for relative in [
        ".config/workspace.json",
        "tasks/tasks.csv",
        "tasks/habits.csv",
        "RCLONE_TEST",
        "stable.md",
    ] {
        let destination = remote.join(relative);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::copy(source.join(relative), destination).unwrap();
    }
}

fn write_tasks(root: &Path, text: &str) {
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(root.join("tasks/tasks.csv"), text).unwrap();
}

fn write_registry(config_home: &Path, root: &Path) {
    std::fs::create_dir_all(config_home.join("brain")).unwrap();
    let registry = serde_json::json!({
        "schema_version": 2,
        "default_workspace": "family",
        "workspaces": {
            "family": {
                "workspace_id": WORKSPACE_ID,
                "root": root,
                "aliases": [],
                "local_user_id": "pablo",
                "receiver_enabled": false,
                "env": {
                    "sync": {
                        "enabled": true,
                        "b2_bucket": "fixture",
                        "b2_key_id": "fixture-id",
                        "b2_app_key": "fixture-key",
                        "watch": false,
                        "max_delete_percent": 90
                    }
                }
            }
        }
    });
    std::fs::write(
        config_home.join("brain/env.json"),
        format!("{}\n", serde_json::to_string_pretty(&registry).unwrap()),
    )
    .unwrap();
}

fn write_rclone_shim(path: &Path) {
    std::fs::write(
        path,
        br#"#!/bin/sh
set -eu
map_remote() {
  case "$1" in
    BRAIN:*/*) printf '%s/%s' "$REMOTE_ROOT" "${1#BRAIN:*/}" ;;
    BRAIN:*) printf '%s' "$REMOTE_ROOT" ;;
    *) printf '%s' "$1" ;;
  esac
}
command="$1"
shift
case "$command" in
  version) exec "$REAL_RCLONE" version "$@" ;;
  cat|lsf|mkdir|delete|deletefile)
    target="$(map_remote "$1")"
    shift
    exec "$REAL_RCLONE" "$command" "$target" "$@"
    ;;
  copyto)
    source="$(map_remote "$1")"
    destination="$(map_remote "$2")"
    shift 2
    exec "$REAL_RCLONE" copyto "$source" "$destination" "$@"
    ;;
  bisync)
    left="$(map_remote "$1")"
    right="$(map_remote "$2")"
    shift 2
    exec "$REAL_RCLONE" bisync "$left" "$right" "$@"
    ;;
  *) exec "$REAL_RCLONE" "$command" "$@" ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

fn find_rclone() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("rclone"))
        .find(|candidate| candidate.is_file())
        .expect("rclone path")
}

fn workspace_id() -> WorkspaceId {
    WorkspaceId::parse(WORKSPACE_ID).unwrap()
}

fn task_rows(root: &Path) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut reader = csv::Reader::from_path(root.join("tasks/tasks.csv")).unwrap();
    let headers = reader.headers().unwrap().clone();
    reader
        .records()
        .map(|record| {
            let record = record.unwrap();
            let row = headers
                .iter()
                .zip(record.iter())
                .map(|(column, value)| (column.to_owned(), value.to_owned()))
                .collect::<BTreeMap<_, _>>();
            (row["task_id"].clone(), row)
        })
        .collect()
}
