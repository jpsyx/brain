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
    assert!(selected.record().root.is_dir());
    let manifest = WorkspaceManifest::load(&selected.record().root, env!("CARGO_PKG_VERSION"))
        .expect("legacy migration creates the portable manifest");
    assert_eq!(manifest.workspace_id(), selected.record().workspace_id);
    assert_eq!(outcome.registry.workspaces.len(), 1);
    assert!(selected.record().aliases.is_empty());
    assert!(selected.record().local_user_id.is_empty());
    assert!(selected.record().receiver_enabled);
    assert_eq!(selected.record().env["claude_cmd"], "claude --legacy");
    assert_eq!(selected.record().env["codex_cmd"], "codex --legacy");
    // Machine-scoped: these land in the registry's global map, never in a
    // record. The receiver origin is one of them, because one machine serves one
    // `/sms` and one `/email` URL for every workspace on it.
    assert_eq!(
        outcome.registry.env["markdown_to_pdf_path"],
        "/opt/bin/markdown-to-pdf"
    );
    assert_eq!(
        outcome.registry.env["brain_receiver_public_url"],
        "https://brain.example.test"
    );
    for machine_key in ["markdown_to_pdf_path", "brain_receiver_public_url"] {
        assert!(
            !selected.record().env.contains_key(machine_key),
            "a machine-global value must not be siloed into a workspace record"
        );
    }
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
    // Four workspace-scoped keys; the two machine-scoped ones went global.
    assert_eq!(selected.record().env.len(), 4);
    assert_eq!(outcome.registry.env.len(), 2);
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
