#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const WORKSPACE_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

#[test]
fn sender_mapping_pulled_by_final_sync_is_preflighted_before_rollout_mutation() {
    let fixture = Fixture::new(
        r#"{"enable_triage_habits":false,"allowed_email_senders":"relative@example.test"}
"#,
    );
    let legacy_tasks = std::fs::read(fixture.root.join("tasks/tasks.csv")).unwrap();

    let output = fixture.run();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success(), "migration unexpectedly succeeded");
    assert!(
        stderr.contains("brain user update <USER_ID> -w family --add-email relative@example.test"),
        "{stderr}"
    );
    assert_eq!(
        std::fs::read(fixture.root.join("tasks/tasks.csv")).unwrap(),
        legacy_tasks,
        "post-sync mapping preflight must finish before task cutover"
    );
    assert!(
        !fixture.cache().join("migration-backups").exists(),
        "post-sync mapping refusal must precede durable backup mutation"
    );
}

#[test]
fn triage_config_pulled_by_final_sync_does_not_poison_migration_resume() {
    let fixture = Fixture::new(
        r#"{"enable_triage_habits":false}
"#,
    );

    let first = fixture.run();
    let second = fixture.run();
    assert!(
        second.status.success(),
        "first run:\n{}\nsecond run:\n{}",
        String::from_utf8_lossy(&first.stderr),
        String::from_utf8_lossy(&second.stderr)
    );
    let habits = std::fs::read_to_string(fixture.root.join("tasks/habits.csv")).unwrap();
    assert!(!habits.contains("brain.triage."), "{habits}");
    assert!(
        !fixture
            .cache()
            .join("migrations/multi-workspace-v1.json")
            .exists(),
        "successful resume must remove the active journal"
    );
}

struct Fixture {
    temporary: tempfile::TempDir,
    home: PathBuf,
    config_home: PathBuf,
    root: PathBuf,
    bin: PathBuf,
}

impl Fixture {
    fn new(pulled_config: &str) -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path().join("home");
        let config_home = temporary.path().join("machine-config");
        let root = temporary.path().join("workspace");
        let bin = temporary.path().join("bin");
        std::fs::create_dir_all(config_home.join("brain")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        write_workspace(&root);
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
                            "b2_bucket": "migration-fixture",
                            "b2_key_id": "fixture-id",
                            "b2_app_key": "fixture-key",
                            "watch": false
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
        let pulled_config_path = temporary.path().join("pulled-config.json");
        std::fs::write(&pulled_config_path, pulled_config).unwrap();
        let remote_tasks = temporary.path().join("remote-tasks.csv");
        let remote_habits = temporary.path().join("remote-habits.csv");
        std::fs::copy(root.join("tasks/tasks.csv"), &remote_tasks).unwrap();
        std::fs::copy(root.join("tasks/habits.csv"), &remote_habits).unwrap();
        let remote_manifest = temporary.path().join("remote-workspace.json");
        std::fs::copy(
            brain::workspace::WorkspaceManifest::path(&root),
            &remote_manifest,
        )
        .unwrap();
        write_fake_rclone(&bin.join("rclone"));
        std::fs::write(
            temporary.path().join("fixture-env"),
            format!(
                "{}\n{}\n{}\n{}\n",
                pulled_config_path.display(),
                remote_tasks.display(),
                remote_habits.display(),
                remote_manifest.display()
            ),
        )
        .unwrap();
        Self {
            temporary,
            home,
            config_home,
            root,
            bin,
        }
    }

    fn run(&self) -> Output {
        let mut paths = std::fs::read_to_string(self.temporary.path().join("fixture-env"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 4);
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args([
                "workspace",
                "migrate",
                "-b",
                "family",
                "--acknowledge-all-machines-updated",
            ])
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.config_home)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("NO_COLOR", "1")
            .env("PULLED_CONFIG", paths.remove(0))
            .env("REMOTE_TASKS", paths.remove(0))
            .env("REMOTE_HABITS", paths.remove(0))
            .env("REMOTE_MANIFEST_FILE", paths.remove(0))
            .output()
            .unwrap()
    }

    fn cache(&self) -> PathBuf {
        self.home.join(".cache/brain/workspaces").join(WORKSPACE_ID)
    }
}

fn write_workspace(root: &Path) {
    brain::workspace::WorkspaceManifest::new(
        brain::workspace::WorkspaceId::parse(WORKSPACE_ID).unwrap(),
    )
    .write_new(root)
    .unwrap();
    std::fs::write(
        root.join(".config/users.json"),
        b"{\"schema_version\":1,\"users\":[{\"id\":\"pablo\",\"name\":\"Pablo\",\"phones\":[],\"emails\":[],\"response_email\":null}]}\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".config/config.json"),
        b"{\"enable_triage_habits\":true}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tasks/tasks.csv"),
        b"task_id,task_name,assigned_to\nT1,Plan,pablo\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tasks/habits.csv"),
        b"task_id,task_name,assigned_to\nH1,Walk,pablo\n",
    )
    .unwrap();
    std::fs::write(root.join("tasks/SCHEMA.json"), b"{}\n").unwrap();
}

fn write_fake_rclone(path: &Path) {
    std::fs::write(
        path,
        b"#!/bin/sh\ncase \"$1\" in\n  version) exit 0 ;;\n  cat) cat \"$REMOTE_MANIFEST_FILE\"; exit 0 ;;\n  bisync) mkdir -p \"$2/.config\"; cp \"$PULLED_CONFIG\" \"$2/.config/config.json\"; exit 0 ;;\n  copyto)\n    case \"$2\" in\n      BRAIN:*/tasks/tasks.csv) cp \"$REMOTE_TASKS\" \"$3\"; exit 0 ;;\n      BRAIN:*/tasks/habits.csv) cp \"$REMOTE_HABITS\" \"$3\"; exit 0 ;;\n      BRAIN:*) exit 1 ;;\n      *) exit 0 ;;\n    esac\n    ;;\n  *) exit 0 ;;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
