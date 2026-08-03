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
    assert!(stderr.contains("brain user add -b brain --id <USER_ID> --name <DISPLAY_NAME>"));
    assert!(stderr.contains("brain user local <USER_ID> -b brain"));
    assert!(
        root.is_dir(),
        "manifest migration must create the workspace root"
    );
}
