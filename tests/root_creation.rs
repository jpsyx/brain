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
fn sync_creates_a_missing_configured_root() {
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

    let output = brain_command(&home, &config_home)
        .arg("sync")
        .output()
        .expect("run brain sync");

    assert!(output.status.success(), "sync failed: {output:?}");
    assert!(root.is_dir(), "sync did not create {}", root.display());
}

#[test]
fn env_list_does_not_create_the_brain_root() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    let root = home.path().join("brain");
    assert!(!root.exists());

    let output = brain_command(&home, &config_home)
        .args(["env", "list"])
        .output()
        .expect("run brain env list");

    assert!(output.status.success(), "env list failed: {output:?}");
    assert!(
        !root.exists(),
        "env list unexpectedly created the brain root"
    );
}
