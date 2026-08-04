use super::*;

fn settings_hook_commands(settings_path: &Path, event: &str) -> Vec<String> {
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(settings_path).expect("read installed settings"))
            .expect("parse installed settings");
    settings["hooks"][event]
        .as_array()
        .expect("hook event array")
        .iter()
        .flat_map(|entry| {
            entry["hooks"]
                .as_array()
                .expect("hook command array")
                .iter()
        })
        .map(|hook| {
            hook["command"]
                .as_str()
                .expect("hook command string")
                .to_owned()
        })
        .collect()
}

#[test]
fn installer_uses_the_explicit_selected_root_and_relative_project_commands() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let home = temp.path().join("home");
    let selected_root = temp.path().join("family");
    let ignored_env_root = temp.path().join("ignored-env-root");
    std::fs::create_dir_all(&home).expect("create home");

    let output = Command::new("bash")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/install_hook.sh"))
        .arg(&selected_root)
        .env("HOME", &home)
        .env("BRAIN_ROOT", &ignored_env_root)
        .output()
        .expect("run hook installer");

    assert!(
        output.status.success(),
        "installer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let settings = selected_root.join(".claude/settings.json");
    assert_eq!(
        settings_hook_commands(&settings, "SessionStart"),
        vec!["python3 .claude/brain-hooks/claude_session_start_hook.py"]
    );
    assert_eq!(
        settings_hook_commands(&settings, "Stop"),
        vec!["python3 .claude/brain-hooks/claude_stop_hook.py"]
    );
    assert!(
        selected_root
            .join(".claude/brain-hooks/claude_session_start_hook.py")
            .is_file()
    );
    assert!(!ignored_env_root.exists());
    assert!(!home.join("brain").exists());
}

#[test]
fn installer_uses_brain_root_when_no_explicit_root_is_passed() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let home = temp.path().join("home");
    let selected_root = temp.path().join("family-from-env");
    std::fs::create_dir_all(&home).expect("create home");

    let output = Command::new("bash")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/install_hook.sh"))
        .env("HOME", &home)
        .env("BRAIN_ROOT", &selected_root)
        .output()
        .expect("run hook installer");

    assert!(
        output.status.success(),
        "installer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(selected_root.join(".claude/settings.json").is_file());
    assert!(!home.join("brain").exists());
}
