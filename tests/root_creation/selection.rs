use super::*;

/// `brain` (default) and `family`, each with a distinguishable config value.
pub(super) fn two_workspaces(home: &TempDir, config_home: &TempDir) {
    let env_dir = config_home.path().join("brain");
    std::fs::create_dir_all(&env_dir).expect("env dir");
    let mut workspaces = serde_json::Map::new();
    for (name, id) in [
        ("brain", "dfbc1768-fcd3-4c74-916f-71289ec2cb7e"),
        ("family", "8d7d67d6-63fc-4d99-8ff9-ebe31ac93fed"),
    ] {
        let root = home.path().join(name);
        std::fs::create_dir_all(root.join(".config")).expect("root");
        std::fs::write(
            root.join(".config/config.json"),
            format!("{{\"linear_workspace\":\"{name}-slug\"}}\n"),
        )
        .expect("config");
        workspaces.insert(
            name.to_owned(),
            serde_json::json!({
                "workspace_id": id,
                "root": root,
                "aliases": [],
                "local_user_id": "pablo",
                "receiver_enabled": false,
                "env": {}
            }),
        );
    }
    std::fs::write(
        env_dir.join("env.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": brain::workspace::REGISTRY_SCHEMA_VERSION,
            "default_workspace": "brain",
            "workspaces": workspaces,
        }))
        .expect("serialize registry"),
    )
    .expect("write registry");
}

#[test]
fn standing_inside_a_workspace_selects_it_the_way_git_finds_a_repo() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    two_workspaces(&home, &config_home);
    let deep = home.path().join("family/projects/work__thing");
    std::fs::create_dir_all(&deep).expect("nested working directory");

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["config", "get", "linear_workspace"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .env_remove("BRAIN_WORKSPACE")
        .current_dir(&deep)
        .output()
        .expect("run from inside a workspace");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "family-slug",
        "a command run inside ~/family must act on family"
    );
}

#[test]
fn the_launching_workspace_outranks_the_current_directory() {
    // An agent panel opened for `brain` stays on `brain` even while it reads
    // files under another workspace's root.
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    two_workspaces(&home, &config_home);

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["config", "get", "linear_workspace"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .env("BRAIN_WORKSPACE", "brain")
        .current_dir(home.path().join("family"))
        .output()
        .expect("run with a launching workspace");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "brain-slug");
}

#[test]
fn standing_outside_every_workspace_still_uses_the_default() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    two_workspaces(&home, &config_home);
    let elsewhere = home.path().join("src/unrelated");
    std::fs::create_dir_all(&elsewhere).expect("unrelated directory");

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["config", "get", "linear_workspace"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .env_remove("BRAIN_WORKSPACE")
        .current_dir(&elsewhere)
        .output()
        .expect("run from outside every workspace");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "brain-slug");
}

#[test]
fn an_explicit_selector_beats_the_current_directory() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    two_workspaces(&home, &config_home);

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["config", "get", "linear_workspace", "-w", "brain"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .env_remove("BRAIN_WORKSPACE")
        .current_dir(home.path().join("family"))
        .output()
        .expect("run with an explicit selector");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "brain-slug");
}
