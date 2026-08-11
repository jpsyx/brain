use std::process::{Command, Output};

use tempfile::TempDir;

struct Fixture {
    home: TempDir,
    config_home: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().expect("isolated HOME"),
            config_home: tempfile::tempdir().expect("isolated XDG_CONFIG_HOME"),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("NO_COLOR", "1")
            .output()
            .expect("run brain")
    }
}

/// A workspace can be non-empty and still have no task store: a joining machine
/// that pulled content before this was fixed looks exactly like that. Seeding
/// only empty workspaces would leave those stuck, so the task store is ensured
/// unconditionally.
#[test]
fn a_nonempty_workspace_missing_its_task_store_still_gets_one() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert!(
        fixture
            .run(&["workspace", "create", "--root", family.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        fixture
            .run(&[
                "workspace",
                "repair",
                "--local-user-id",
                "pablo",
                "-w",
                "family",
            ])
            .status
            .success()
    );
    assert!(
        fixture
            .run(&["tasks", "today", "--no-tui", "-w", "family"])
            .status
            .success()
    );
    // Content elsewhere makes the root non-empty; the task store is gone.
    std::fs::write(family.join("areas/note.md"), b"kept").unwrap();
    std::fs::remove_dir_all(family.join("tasks")).unwrap();

    assert!(
        fixture
            .run(&["tasks", "today", "--no-tui", "-w", "family"])
            .status
            .success()
    );

    for path in [
        "tasks/SCHEMA.json",
        "tasks/tasks.csv",
        "tasks/habits.csv",
        "tasks/.tasks_next_id",
        "tasks/.habits_next_id",
    ] {
        assert!(family.join(path).is_file(), "missing {path}");
    }
    assert_eq!(std::fs::read(family.join("areas/note.md")).unwrap(), b"kept");
}

/// A machine joining a workspace must have its local task store before the
/// first sync, because the sync's CSV lane reads `tasks/SCHEMA.json` to decide
/// how to merge. Seeding it afterwards meant a fresh machine synced as `Legacy`
/// against a `Current` remote and refused, leaving `tasks/` entirely empty — so
/// even the suggested `brain workspace migrate` had nothing to read.
#[test]
fn the_local_task_store_exists_even_when_the_first_sync_fails() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert!(
        fixture
            .run(&["workspace", "create", "--root", family.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        fixture
            .run(&[
                "workspace",
                "repair",
                "--local-user-id",
                "pablo",
                "-w",
                "family",
            ])
            .status
            .success()
    );
    // Configured sync that cannot possibly succeed, written directly because
    // `brain env set` deliberately refuses to persist credentials it cannot
    // reach. The startup sync then fails exactly where the real one refused.
    let env_path = fixture.config_home.path().join("brain/env.json");
    let mut env: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&env_path).unwrap()).unwrap();
    env["workspaces"]["family"]["env"]["sync"] = serde_json::json!({
        "enabled": true,
        "b2_bucket": "brain-test-unreachable-bucket",
        "b2_key_id": "0000000000000000000000000",
        "b2_app_key": "unusable-application-key",
        "b2_path": "",
    });
    std::fs::write(&env_path, serde_json::to_vec_pretty(&env).unwrap()).unwrap();
    std::fs::remove_dir_all(family.join("tasks")).ok();

    let _ = fixture.run(&["tasks", "today", "--no-tui", "-w", "family"]);

    for path in [
        "tasks/SCHEMA.json",
        "tasks/tasks.csv",
        "tasks/habits.csv",
        "tasks/.tasks_next_id",
        "tasks/.habits_next_id",
    ] {
        assert!(
            family.join(path).is_file(),
            "a failed first sync must still leave {path} in place"
        );
    }
}

/// Sync subcommands dispatch *before* the workspace gate, so they never reach
/// root initialization. `brain sync setup` is precisely the command that cannot
/// run without the task schema document, so the sync entry point has to seed it
/// too — a workspace created before Brain shipped the document would otherwise
/// keep failing setup no matter how many times it was retried.
#[test]
fn a_sync_command_seeds_the_task_schema_document_it_needs() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert!(
        fixture
            .run(&["workspace", "create", "--root", family.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        fixture
            .run(&[
                "workspace",
                "repair",
                "--local-user-id",
                "pablo",
                "-w",
                "family",
            ])
            .status
            .success()
    );
    // Initialize fully, then take the document away: the shape of a workspace
    // created before Brain shipped it, which is where this was found.
    assert!(
        fixture
            .run(&["tasks", "today", "--no-tui", "-w", "family"])
            .status
            .success()
    );
    let schema = family.join("tasks/SCHEMA.json");
    std::fs::remove_file(&schema).expect("remove the seeded schema document");

    let output = fixture.run(&["sync", "status", "-w", "family"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        schema.is_file(),
        "a sync command must seed the schema document it requires"
    );
}

#[test]
fn first_tasks_command_initializes_an_empty_workspace() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert!(
        fixture
            .run(&["workspace", "create", "--root", family.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        fixture
            .run(&[
                "workspace",
                "repair",
                "--local-user-id",
                "pablo",
                "-w",
                "family",
            ])
            .status
            .success()
    );

    let output = fixture.run(&["tasks", "today", "--no-tui", "-w", "family"]);

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for path in [
        ".config/config.json",
        "tasks/tasks.csv",
        "tasks/habits.csv",
        "tasks/.tasks_next_id",
        "tasks/.habits_next_id",
        "tasks/SCHEMA.json",
        "projects/projects-lookup.csv",
        "resources/zotero-lookup.csv",
    ] {
        assert!(family.join(path).is_file(), "missing {path}");
    }
    for directory in ["projects", "areas", "resources", "archive", "tasks"] {
        assert!(family.join(directory).is_dir(), "missing {directory}");
    }

    // Without this document every schema decision fails, so a workspace Brain
    // just created could not complete `brain sync setup`.
    let schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(family.join("tasks/SCHEMA.json")).unwrap()).unwrap();
    assert_eq!(schema["task_schema_version"].as_u64(), Some(2));
    assert_eq!(schema["merge_key"].as_str(), Some("task_uuid"));
}
