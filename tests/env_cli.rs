use std::process::Command;

use serde_json::json;
use tempfile::TempDir;

fn brain_command(home: &TempDir, config_home: &TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_brain"));
    command
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1");
    command
}

fn write_env(config_home: &TempDir) -> std::path::PathBuf {
    let env_dir = config_home.path().join("brain");
    std::fs::create_dir_all(&env_dir).expect("env dir");
    let path = env_dir.join("env.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "root": "~/brain",
            "sync": {
                "enabled": true,
                "remote": {"bucket": "pablo-brain", "credentials": {"key_id": "abc"}}
            }
        }))
        .expect("serialize env"),
    )
    .expect("write env");
    path
}

#[test]
fn env_list_get_and_set_support_recursive_dotted_paths() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    let env_path = write_env(&config_home);

    let listed = brain_command(&home, &config_home)
        .args(["env", "list"])
        .output()
        .expect("env list");
    assert!(listed.status.success());
    let stdout = String::from_utf8_lossy(&listed.stdout);
    assert!(
        stdout.contains("sync.remote.credentials.key_id"),
        "{stdout}"
    );
    assert!(stdout.contains("sync.enabled"), "{stdout}");

    let got = brain_command(&home, &config_home)
        .args(["env", "get", "sync.remote.credentials.key_id"])
        .output()
        .expect("env get");
    assert!(got.status.success());
    assert_eq!(String::from_utf8_lossy(&got.stdout).trim(), "abc");

    let set = brain_command(&home, &config_home)
        .args(["env", "set", "sync.remote.credentials.key_id=updated"])
        .output()
        .expect("env set");
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );

    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(env_path).expect("read env"))
            .expect("parse env");
    assert_eq!(saved["sync"]["enabled"], true);
    assert_eq!(saved["sync"]["remote"]["bucket"], "pablo-brain");
    assert_eq!(saved["sync"]["remote"]["credentials"]["key_id"], "updated");
}
