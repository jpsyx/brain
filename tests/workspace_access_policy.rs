use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use brain::access::AccessMode;
use brain::config::Config;
use brain::workspace::{RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName};

struct CliFixture {
    home: tempfile::TempDir,
    config_home: tempfile::TempDir,
    current_dir: tempfile::TempDir,
}

impl CliFixture {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().expect("isolated HOME"),
            config_home: tempfile::tempdir().expect("isolated XDG_CONFIG_HOME"),
            current_dir: tempfile::tempdir().expect("isolated current directory"),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(args)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.config_home.path())
            .env("NO_COLOR", "1")
            .current_dir(self.current_dir.path())
            .output()
            .expect("run brain")
    }

    fn registry_path(&self) -> PathBuf {
        self.config_home.path().join("brain/env.json")
    }

    fn make_ready(&self, workspace: &str) {
        assert_success(&self.run(&[
            "workspace",
            "repair",
            "--local-user-id",
            "test-user",
            "-b",
            workspace,
        ]));
    }
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("fixture paths are UTF-8")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn workspace(root: &std::path::Path) -> WorkspaceContext {
    WorkspaceContext::new(
        root.parent().expect("fixture root has a parent"),
        WorkspaceId::new(),
        WorkspaceName::parse("family").expect("valid workspace name"),
        root,
        "pablo",
        root.parent().expect("fixture root has a parent"),
    )
    .expect("workspace context")
}

fn stored_mode(root: &std::path::Path) -> Option<&'static str> {
    let stored: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(".config/config.json")).expect("stored portable config"),
    )
    .expect("portable config JSON");
    match stored
        .get("access_mode")
        .and_then(serde_json::Value::as_str)
    {
        Some("unrestricted") => Some("unrestricted"),
        Some("workspace_only") => Some("workspace_only"),
        _ => None,
    }
}

#[test]
fn portable_config_defaults_to_unrestricted_and_parses_workspace_only() {
    let default: Config = serde_json::from_str("{}").expect("default portable config");
    let restricted: Config = serde_json::from_str(r#"{"access_mode":"workspace_only"}"#)
        .expect("workspace-only portable config");

    assert_eq!(default.access_mode, AccessMode::Unrestricted);
    assert_eq!(restricted.access_mode, AccessMode::WorkspaceOnly);
}

#[test]
fn trusted_config_command_persists_a_valid_access_mode() {
    let fixture = tempfile::tempdir().expect("temporary workspace parent");
    let root = fixture.path().join("family");
    std::fs::create_dir_all(root.join(".config")).expect("portable config directory");
    let workspace = workspace(&root);

    brain::settings::set(&workspace, "access_mode", "workspace_only")
        .expect("trusted config mutation");

    assert_eq!(
        Config::load(&workspace).access_mode,
        AccessMode::WorkspaceOnly
    );
    let stored: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(".config/config.json")).expect("stored portable config"),
    )
    .expect("portable config JSON");
    assert_eq!(stored["access_mode"], "workspace_only");
}

#[test]
fn trusted_config_command_rejects_unknown_access_modes_without_mutation() {
    let fixture = tempfile::tempdir().expect("temporary workspace parent");
    let root = fixture.path().join("family");
    std::fs::create_dir_all(root.join(".config")).expect("portable config directory");
    std::fs::write(
        root.join(".config/config.json"),
        b"{\"access_mode\":\"workspace_only\",\"keep\":true}\n",
    )
    .expect("initial portable config");
    let workspace = workspace(&root);
    let before = std::fs::read(root.join(".config/config.json")).expect("config before mutation");

    let error = brain::settings::set(&workspace, "access_mode", "isolated")
        .expect_err("unknown access mode must be rejected");

    assert!(
        error
            .to_string()
            .contains("access_mode must be unrestricted or workspace_only")
    );
    assert_eq!(
        std::fs::read(root.join(".config/config.json")).expect("config after rejection"),
        before
    );
}

#[test]
fn legacy_migration_persists_unrestricted_without_overwriting_portable_config() {
    let home = tempfile::tempdir().expect("temporary home");
    let machine = tempfile::tempdir().expect("temporary machine config");
    let config_dir = machine.path().join("brain");
    std::fs::create_dir_all(&config_dir).expect("machine config directory");
    let root = home.path().join("personal");
    std::fs::create_dir_all(root.join(".config")).expect("portable config directory");
    std::fs::write(root.join(".config/config.json"), b"{\"keep\":\"yes\"}\n")
        .expect("legacy portable config");
    let legacy = br#"{"root":"~/personal","claude_cmd":"claude"}"#;

    brain::workspace::migrate_legacy(home.path(), &config_dir, legacy).expect("legacy migration");

    let stored: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(".config/config.json")).expect("migrated portable config"),
    )
    .expect("portable config JSON");
    assert_eq!(stored_mode(&root), Some("unrestricted"));
    assert_eq!(stored["keep"], "yes");
}

#[test]
fn migration_does_not_publish_registry_when_access_mode_cannot_be_persisted() {
    let home = tempfile::tempdir().expect("temporary home");
    let machine = tempfile::tempdir().expect("temporary machine config");
    let config_dir = machine.path().join("brain");
    std::fs::create_dir_all(&config_dir).expect("machine config directory");
    let root = home.path().join("personal");
    std::fs::create_dir_all(root.join(".config/config.json"))
        .expect("config path blocked by a directory");
    let legacy = br#"{"root":"~/personal"}"#;

    brain::workspace::migrate_legacy(home.path(), &config_dir, legacy)
        .expect_err("portable config failure must abort migration");

    assert!(
        !config_dir.join("env.json").exists(),
        "registry must not publish before portable access mode"
    );
}

#[test]
fn first_created_workspace_is_unrestricted_and_later_workspaces_are_workspace_only() {
    let fixture = CliFixture::new();
    let personal = fixture.home.path().join("personal");
    let family = fixture.home.path().join("family");

    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&personal)]));
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));

    assert_eq!(stored_mode(&personal), Some("unrestricted"));
    assert_eq!(stored_mode(&family), Some("workspace_only"));
}

#[test]
fn changing_machine_default_does_not_rewrite_portable_access_modes() {
    let fixture = CliFixture::new();
    let personal = fixture.home.path().join("personal");
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&personal)]));
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    fixture.make_ready("personal");
    fixture.make_ready("family");
    let personal_before =
        std::fs::read(personal.join(".config/config.json")).expect("personal config");
    let family_before = std::fs::read(family.join(".config/config.json")).expect("family config");

    assert_success(&fixture.run(&["workspace", "default", "family"]));

    let registry = RegistryStore::load_from(&fixture.registry_path()).expect("machine registry");
    assert_eq!(registry.default_workspace.as_str(), "family");
    assert_eq!(
        std::fs::read(personal.join(".config/config.json")).expect("personal config after"),
        personal_before
    );
    assert_eq!(
        std::fs::read(family.join(".config/config.json")).expect("family config after"),
        family_before
    );
}
