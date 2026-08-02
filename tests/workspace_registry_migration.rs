use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use brain::workspace::{
    MachineRegistry, REGISTRY_SCHEMA_VERSION, RegistryStore, WorkspaceId, WorkspaceName,
    WorkspaceRecord, migrate_legacy,
};
use serde_json::json;

fn run_migration(home: &std::path::Path, config_home: &std::path::Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["env", "list"])
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config_home)
        .env("NO_COLOR", "1")
        .output()
        .expect("run brain migration");
    assert!(
        output.status.success(),
        "migration command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture_dirs() -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().expect("home fixture");
    let config_home = tempfile::tempdir().expect("config fixture");
    let config_dir = config_home.path().join("brain");
    fs::create_dir_all(&config_dir).expect("create machine config");
    (home, config_home, config_dir)
}

fn backup_files(config_dir: &Path) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(config_dir)
        .expect("read machine config")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("env.json.legacy-backup"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[test]
fn startup_migrates_the_flat_env_into_one_default_workspace() {
    let home = tempfile::tempdir().expect("home fixture");
    let config_home = tempfile::tempdir().expect("config fixture");
    let machine_config = config_home.path().join("brain");
    fs::create_dir_all(&machine_config).expect("create machine config");
    let env_path = machine_config.join("env.json");
    let legacy = serde_json::to_vec_pretty(&json!({
        "root": "~/notes/./brain",
        "claude_cmd": "claude --legacy",
        "codex_cmd": "codex --legacy",
        "receiver_enabled": true,
        "brain_receiver_public_url": "https://brain.example.test",
        "markdown_to_pdf_path": "/opt/bin/markdown-to-pdf",
        "custom_machine_key": ["keep", {"nested": true}],
        "sync": {"enabled": true, "b2_bucket": "fixture-bucket"}
    }))
    .expect("serialize legacy fixture");
    fs::write(&env_path, &legacy).expect("write legacy env");

    run_migration(home.path(), config_home.path());

    let registry: MachineRegistry =
        serde_json::from_slice(&fs::read(&env_path).expect("read migrated registry"))
            .expect("schema-v2 registry");
    let selected = registry.select(None).expect("default workspace");
    assert_eq!(selected.canonical_name().as_str(), "brain");
    assert_eq!(selected.record().root, home.path().join("notes/brain"));
    assert_eq!(
        selected.record().env["sync"],
        json!({"enabled": true, "b2_bucket": "fixture-bucket"})
    );
    assert_eq!(registry.default_workspace.as_str(), "brain");
}

#[test]
fn flat_bytes_are_backed_up_exactly_and_machine_keys_are_siloed_losslessly() {
    let (home, _config_home, config_dir) = fixture_dirs();
    let env_path = config_dir.join("env.json");
    let legacy = br#"{
  "root": "~/Brain_Data/./notes/..",
  "claude_cmd": "claude --legacy",
  "codex_cmd": "codex --legacy",
  "receiver_enabled": true,
  "brain_receiver_public_url": "https://brain.example.test",
  "markdown_to_pdf_path": "/opt/bin/markdown-to-pdf",
  "custom_machine_key": ["keep", {"nested": true}],
  "sync": {"enabled": true, "remote": {"bucket": "fixture"}},
  "access_mode": "unrestricted",
  "access_policy": {"allow": "all"}
}
"#;
    fs::write(&env_path, legacy).expect("write legacy env");

    let outcome = migrate_legacy(home.path(), &config_dir, legacy).expect("migrate flat env");

    let selected = outcome.registry.select(None).expect("default workspace");
    assert_eq!(selected.canonical_name().as_str(), "brain_data");
    assert_eq!(selected.record().root, home.path().join("Brain_Data"));
    assert!(selected.record().root.is_absolute());
    assert!(!selected.record().root.exists());
    assert_eq!(outcome.registry.workspaces.len(), 1);
    assert!(selected.record().aliases.is_empty());
    assert!(selected.record().local_user_id.is_empty());
    assert!(selected.record().receiver_enabled);
    assert_eq!(selected.record().env["claude_cmd"], "claude --legacy");
    assert_eq!(selected.record().env["codex_cmd"], "codex --legacy");
    assert_eq!(
        selected.record().env["brain_receiver_public_url"],
        "https://brain.example.test"
    );
    assert_eq!(
        selected.record().env["markdown_to_pdf_path"],
        "/opt/bin/markdown-to-pdf"
    );
    assert_eq!(
        selected.record().env["custom_machine_key"],
        json!(["keep", {"nested": true}])
    );
    assert_eq!(
        selected.record().env["sync"],
        json!({"enabled": true, "remote": {"bucket": "fixture"}})
    );
    assert!(!selected.record().env.contains_key("root"));
    assert!(!selected.record().env.contains_key("receiver_enabled"));
    assert!(!selected.record().env.contains_key("access_mode"));
    assert!(!selected.record().env.contains_key("access_policy"));
    assert_eq!(selected.record().env.len(), 6);
    assert!(outcome.created_registry);
    assert!(outcome.portable_setup_required);
    let backup = outcome.backup_path.expect("legacy backup");
    assert_eq!(fs::read(backup).expect("read backup"), legacy);
    let persisted = fs::read_to_string(env_path).expect("read registry");
    assert!(!persisted.contains("access_mode"));
    assert!(!persisted.contains("access_policy"));
}

#[test]
fn pointer_only_root_is_normalized_and_the_pointer_is_never_changed() {
    let (home, config_home, config_dir) = fixture_dirs();
    let pointer = config_home.path().join("brain-root");
    let pointer_bytes = b"  ~/Family-Brain/./notes/..  \n";
    fs::write(&pointer, pointer_bytes).expect("write legacy pointer");

    let outcome = migrate_legacy(home.path(), &config_dir, b"").expect("migrate pointer");
    let selected = outcome.registry.select(None).expect("default workspace");

    assert_eq!(selected.canonical_name().as_str(), "family-brain");
    assert_eq!(selected.record().root, home.path().join("Family-Brain"));
    assert_eq!(fs::read(&pointer).expect("read pointer"), pointer_bytes);
    assert!(pointer.exists());
}

#[test]
fn no_prior_files_creates_the_default_brain_registry_without_a_backup() {
    let (home, _config_home, config_dir) = fixture_dirs();

    let outcome = migrate_legacy(home.path(), &config_dir, b"").expect("create registry");
    let selected = outcome.registry.select(None).expect("default workspace");

    assert_eq!(selected.canonical_name().as_str(), "brain");
    assert_eq!(selected.record().root, home.path().join("brain"));
    assert!(outcome.created_registry);
    assert!(outcome.portable_setup_required);
    assert_eq!(outcome.backup_path, None);
    assert!(backup_files(&config_dir).is_empty());
    assert!(!config_dir.parent().unwrap().join("brain-root").exists());
}

#[test]
fn invalid_and_non_object_legacy_json_migrate_as_empty_env() {
    for legacy in [b"not json".as_slice(), b"[1, 2, 3]".as_slice()] {
        let (home, _config_home, config_dir) = fixture_dirs();
        fs::write(config_dir.join("env.json"), legacy).expect("write broken env");

        let outcome = migrate_legacy(home.path(), &config_dir, legacy).expect("migrate broken env");
        let selected = outcome.registry.select(None).expect("default workspace");

        assert_eq!(selected.record().root, home.path().join("brain"));
        assert!(selected.record().env.is_empty());
        assert_eq!(
            fs::read(outcome.backup_path.expect("broken legacy backup")).expect("read backup"),
            legacy
        );
    }
}

#[test]
fn invalid_root_basename_falls_back_to_brain_canonical_name() {
    let (home, _config_home, config_dir) = fixture_dirs();
    let legacy = br#"{"root":"~/Family Notes"}"#;

    let outcome = migrate_legacy(home.path(), &config_dir, legacy).expect("migrate invalid name");
    let selected = outcome.registry.select(None).expect("default workspace");

    assert_eq!(selected.canonical_name().as_str(), "brain");
    assert_eq!(selected.record().root, home.path().join("Family Notes"));
}

#[test]
fn rerun_keeps_the_uuid_and_registry_bytes_and_creates_no_new_backup() {
    let (home, _config_home, config_dir) = fixture_dirs();
    let env_path = config_dir.join("env.json");
    let legacy = br#"{"root":"~/brain","custom":"keep"}"#;
    fs::write(&env_path, legacy).expect("write legacy env");

    let first = migrate_legacy(home.path(), &config_dir, legacy).expect("first migration");
    let first_id = first
        .registry
        .select(None)
        .expect("default")
        .record()
        .workspace_id;
    let first_bytes = fs::read(&env_path).expect("first registry bytes");
    let first_backups = backup_files(&config_dir);
    let second = migrate_legacy(home.path(), &config_dir, &first_bytes).expect("second migration");

    assert_eq!(
        second
            .registry
            .select(None)
            .expect("default")
            .record()
            .workspace_id,
        first_id
    );
    assert_eq!(
        fs::read(&env_path).expect("second registry bytes"),
        first_bytes
    );
    assert_eq!(backup_files(&config_dir), first_backups);
    assert!(!second.created_registry);
    assert_eq!(second.backup_path, None);
    assert!(!second.portable_setup_required);
}

#[test]
fn valid_schema_v2_registry_is_a_byte_for_byte_no_op() {
    let (home, _config_home, config_dir) = fixture_dirs();
    let env_path = config_dir.join("env.json");
    let canonical = WorkspaceName::parse("existing").expect("valid name");
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical.clone(),
        workspaces: std::collections::BTreeMap::from([(
            canonical,
            WorkspaceRecord {
                workspace_id: WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
                    .expect("valid ID"),
                root: PathBuf::from("/existing/root"),
                aliases: std::collections::BTreeSet::new(),
                local_user_id: "existing-user".to_owned(),
                receiver_enabled: true,
                env: serde_json::Map::from_iter([("custom".to_owned(), json!("keep"))]),
            },
        )]),
    };
    RegistryStore::from_path(env_path.clone())
        .replace(&registry)
        .expect("write valid registry");
    let original = fs::read(&env_path).expect("original bytes");

    let outcome = migrate_legacy(home.path(), &config_dir, &original).expect("check registry");

    assert_eq!(outcome.registry, registry);
    assert_eq!(fs::read(&env_path).expect("unchanged bytes"), original);
    assert!(!outcome.created_registry);
    assert_eq!(outcome.backup_path, None);
    assert!(backup_files(&config_dir).is_empty());
}

#[test]
fn backup_name_uses_the_first_available_deterministic_suffix() {
    let (home, _config_home, config_dir) = fixture_dirs();
    let env_path = config_dir.join("env.json");
    let legacy = br#"{"root":"~/brain"}"#;
    fs::write(&env_path, legacy).expect("write legacy env");
    fs::write(config_dir.join("env.json.legacy-backup"), b"occupied").expect("reserve backup");

    let outcome = migrate_legacy(home.path(), &config_dir, legacy).expect("migrate with collision");

    assert_eq!(
        outcome.backup_path.as_deref(),
        Some(config_dir.join("env.json.legacy-backup.1").as_path())
    );
    assert_eq!(
        fs::read(config_dir.join("env.json.legacy-backup")).unwrap(),
        b"occupied"
    );
    assert_eq!(fs::read(outcome.backup_path.unwrap()).unwrap(), legacy);
}

#[test]
fn startup_relocates_portable_markdown_path_only_after_registry_write() {
    let home = tempfile::tempdir().expect("home fixture");
    let config_home = tempfile::tempdir().expect("config fixture");
    let machine_config = config_home.path().join("brain");
    fs::create_dir_all(&machine_config).expect("create machine config");
    let legacy = br#"{"root":"~/brain","claude_cmd":"claude"}"#;
    fs::write(machine_config.join("env.json"), legacy).expect("write legacy env");
    let portable_config_dir = home.path().join("brain/.config");
    fs::create_dir_all(&portable_config_dir).expect("create portable config");
    let portable_path = portable_config_dir.join("config.json");
    fs::write(
        &portable_path,
        br#"{"markdown_to_pdf_path":"/portable/bin/markdown-to-pdf","keep":"yes"}"#,
    )
    .expect("write portable config");

    run_migration(home.path(), config_home.path());

    let registry =
        RegistryStore::load_from(&machine_config.join("env.json")).expect("load migrated registry");
    assert_eq!(
        registry.select(None).unwrap().record().env["markdown_to_pdf_path"],
        "/portable/bin/markdown-to-pdf"
    );
    let portable: serde_json::Value =
        serde_json::from_slice(&fs::read(&portable_path).expect("read portable config"))
            .expect("parse portable config");
    assert_eq!(portable["keep"], "yes");
    assert!(portable.get("markdown_to_pdf_path").is_none());
}

#[test]
fn failed_registry_replacement_never_removes_the_portable_markdown_path() {
    let home = tempfile::tempdir().expect("home fixture");
    let config_home = tempfile::tempdir().expect("config fixture");
    let machine_config = config_home.path().join("brain");
    fs::create_dir_all(machine_config.join("env.json")).expect("block registry destination");
    let portable_config_dir = home.path().join("brain/.config");
    fs::create_dir_all(&portable_config_dir).expect("create portable config");
    let portable_path = portable_config_dir.join("config.json");
    let portable = br#"{"markdown_to_pdf_path":"/portable/bin/markdown-to-pdf"}"#;
    fs::write(&portable_path, portable).expect("write portable config");

    run_migration(home.path(), config_home.path());

    assert_eq!(
        fs::read(portable_path).expect("portable config remains"),
        portable
    );
}
