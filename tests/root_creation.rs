use std::process::Command;

use tempfile::TempDir;

fn brain_command(home: &TempDir, xdg_config_home: &TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_brain"));
    command
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", xdg_config_home.path())
        .env("NO_COLOR", "1");
    command
}

#[test]
fn readiness_migration_creates_a_missing_configured_root_before_sync() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    let root = home.path().join("nested").join("configured-brain");
    let env_dir = config_home.path().join("brain");
    std::fs::create_dir_all(&env_dir).expect("env dir");
    std::fs::write(
        env_dir.join("env.json"),
        format!(r#"{{"root":"{}"}}"#, root.display()),
    )
    .expect("env config");
    assert!(!root.exists());

    let repair = brain_command(&home, &config_home)
        .args([
            "workspace",
            "repair",
            "--manifest",
            "--local-user-id",
            "test-user",
        ])
        .output()
        .expect("repair migrated workspace");
    assert!(repair.status.success(), "repair failed: {repair:?}");
    assert!(root.is_dir(), "migration did not create {}", root.display());

    let output = brain_command(&home, &config_home)
        .arg("sync")
        .output()
        .expect("run brain sync");

    assert!(output.status.success(), "sync failed: {output:?}");
    assert!(root.is_dir());
}

#[test]
fn first_env_list_creates_the_migrated_root_but_requires_a_portable_local_person() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    let root = home.path().join("brain");
    assert!(!root.exists());

    let output = brain_command(&home, &config_home)
        .args(["env", "list"])
        .output()
        .expect("run brain env list");

    assert!(!output.status.success(), "env list unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("brain user add -w brain --id <USER_ID> --name <DISPLAY_NAME>"));
    assert!(stderr.contains("brain user local <USER_ID> -w brain"));
    assert!(
        root.is_dir(),
        "manifest migration must create the workspace root"
    );
}

/// A machine whose synced `env.json` registers a workspace it has never had.
fn joined_machine(workspace: &str, extra_env: &str) -> (TempDir, TempDir, std::path::PathBuf) {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    let root = home.path().join(workspace);
    let env_dir = config_home.path().join("brain");
    std::fs::create_dir_all(&env_dir).expect("env dir");
    std::fs::write(
        env_dir.join("env.json"),
        format!(
            r#"{{
              "schema_version": {schema},
              "default_workspace": "{workspace}",
              "workspaces": {{
                "{workspace}": {{
                  "workspace_id": "8d7d67d6-63fc-4d99-8ff9-ebe31ac93fed",
                  "root": "{}",
                  "aliases": [],
                  "local_user_id": "pablo",
                  "receiver_enabled": false,
                  "env": {{{extra_env}}}
                }}
              }}
            }}"#,
            root.display(),
            schema = brain::workspace::REGISTRY_SCHEMA_VERSION,
        ),
    )
    .expect("write registry");
    (home, config_home, root)
}

#[test]
fn a_workspace_registered_on_another_machine_is_created_and_seeded_here() {
    // The exact multi-machine case: `env.json` rides between machines, so this
    // machine knows about `family` but has never had the directory.
    let (home, config_home, root) = joined_machine("family", "");
    assert!(!root.exists());

    let output = brain_command(&home, &config_home)
        .args(["config", "get", "day_rollover_hour"])
        .output()
        .expect("run an ordinary command");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.is_dir(), "the workspace root was not created");
    // With no sync configured, setup falls back to seeding PARA and the CSVs.
    for expected in [
        "projects",
        "areas",
        "resources",
        "archive",
        "tasks",
        "tasks/tasks.csv",
        "tasks/habits.csv",
        "tasks/.tasks_next_id",
        "tasks/.habits_next_id",
        "projects/projects-lookup.csv",
        "resources/zotero-lookup.csv",
    ] {
        assert!(root.join(expected).exists(), "missing {expected}");
    }
    // The counters start at 1 and the tables carry their headers, so the very
    // next `brain tasks add` works without any further setup.
    assert_eq!(
        std::fs::read_to_string(root.join("tasks/.tasks_next_id")).expect("counter"),
        "1\n"
    );
    assert!(
        std::fs::read_to_string(root.join("tasks/tasks.csv"))
            .expect("tasks table")
            .starts_with("task_uuid,task_id,task_name"),
    );
}

#[test]
fn a_root_whose_parent_is_missing_is_reported_rather_than_invented() {
    // An unmounted volume looks exactly like a missing root. Creating an empty
    // workspace over it would read as data loss.
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    let root = home.path().join("unmounted-volume").join("family");
    let env_dir = config_home.path().join("brain");
    std::fs::create_dir_all(&env_dir).expect("env dir");
    std::fs::write(
        env_dir.join("env.json"),
        format!(
            r#"{{"schema_version":{schema},"default_workspace":"family","workspaces":{{"family":{{"workspace_id":"8d7d67d6-63fc-4d99-8ff9-ebe31ac93fed","root":"{}","aliases":[],"local_user_id":"pablo","receiver_enabled":false,"env":{{}}}}}}}}"#,
            root.display(),
            schema = brain::workspace::REGISTRY_SCHEMA_VERSION,
        ),
    )
    .expect("write registry");

    let output = brain_command(&home, &config_home)
        .args(["config", "get", "day_rollover_hour"])
        .output()
        .expect("run an ordinary command");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("parent directory does not exist"),
        "{stderr}"
    );
    assert!(
        !root.exists(),
        "nothing may be created over a missing volume"
    );
}

#[test]
fn setting_up_a_workspace_is_idempotent_and_leaves_content_alone() {
    let (home, config_home, root) = joined_machine("family", "");
    let run = || {
        let output = brain_command(&home, &config_home)
            .args(["config", "get", "day_rollover_hour"])
            .output()
            .expect("run an ordinary command");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run();
    std::fs::write(root.join("areas/notes.md"), b"user content").expect("write user content");
    let tasks_before = std::fs::read(root.join("tasks/tasks.csv")).expect("tasks table");

    run();

    assert_eq!(
        std::fs::read(root.join("tasks/tasks.csv")).expect("tasks table after"),
        tasks_before,
        "re-running setup must not rewrite an existing table"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("areas/notes.md")).expect("user content after"),
        "user content"
    );
}

#[test]
fn a_strict_child_without_a_workspace_selector_is_refused() {
    // Brain sets this on the children it spawns. A code path that builds
    // `brain …` without `-w` must fail loudly rather than sync, reindex, or
    // mutate whichever workspace happens to be the default.
    let (home, config_home, _) = joined_machine("family", "");

    let output = brain_command(&home, &config_home)
        .args(["config", "get", "day_rollover_hour"])
        .env("BRAIN_REQUIRE_WORKSPACE", "1")
        .output()
        .expect("run a strict child");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("must name its workspace explicitly"),
        "{stderr}"
    );
}

#[test]
fn a_strict_child_that_names_its_workspace_runs_normally() {
    let (home, config_home, _) = joined_machine("family", "");

    let output = brain_command(&home, &config_home)
        .args(["config", "get", "day_rollover_hour", "-w", "family"])
        .env("BRAIN_REQUIRE_WORKSPACE", "1")
        .output()
        .expect("run a strict child");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "6");
}

#[test]
fn an_ordinary_invocation_still_uses_the_default_workspace() {
    // Strict mode is for Brain's own children, not for people at a prompt.
    let (home, config_home, _) = joined_machine("family", "");

    let output = brain_command(&home, &config_home)
        .args(["config", "get", "day_rollover_hour"])
        .output()
        .expect("run an ordinary command");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_brain_launched_process_inherits_its_workspace_without_a_selector() {
    // The skill case: a bundled skill runs `brain <command>` inside a `family`
    // agent panel with no `-w`. It must act on `family`, not on the default.
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    two_workspaces(&home, &config_home);

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["config", "get", "linear_workspace"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .env("BRAIN_WORKSPACE", "family")
        .output()
        .expect("run an inherited-workspace command");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "family-slug",
        "the command read the default workspace instead of the launching one"
    );
}

#[test]
fn an_explicit_selector_overrides_the_inherited_workspace() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    two_workspaces(&home, &config_home);

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["config", "get", "linear_workspace", "-w", "brain"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .env("BRAIN_WORKSPACE", "family")
        .output()
        .expect("run with an explicit selector");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "brain-slug");
}

#[test]
fn with_nothing_inherited_a_bare_command_still_uses_the_default() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    two_workspaces(&home, &config_home);

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["config", "get", "linear_workspace"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .env_remove("BRAIN_WORKSPACE")
        .output()
        .expect("run a bare command");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "brain-slug");
}

#[path = "root_creation/selection.rs"]
mod selection;

use selection::two_workspaces;
