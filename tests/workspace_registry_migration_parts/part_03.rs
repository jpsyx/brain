
#[test]
fn valid_schema_v2_registry_is_a_byte_for_byte_no_op() {
    let (home, _config_home, config_dir) = fixture_dirs();
    let env_path = config_dir.join("env.json");
    let root = home.path().join("existing");
    fs::create_dir_all(root.join(".config")).unwrap();
    fs::write(
        root.join(".config/config.json"),
        b"{\"access_mode\":\"unrestricted\"}\n",
    )
    .unwrap();
    let canonical = WorkspaceName::parse("existing").expect("valid name");
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical.clone(),
        workspaces: std::collections::BTreeMap::from([(
            canonical,
            WorkspaceRecord {
                workspace_id: WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
                    .expect("valid ID"),
                root,
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
fn valid_schema_v2_registry_seeds_every_missing_portable_access_mode() {
    let (home, _config_home, config_dir) = fixture_dirs();
    let env_path = config_dir.join("env.json");
    let personal = home.path().join("personal");
    let family = home.path().join("family");
    fs::create_dir_all(&personal).unwrap();
    fs::create_dir_all(&family).unwrap();
    let personal_name = WorkspaceName::parse("personal").unwrap();
    let family_name = WorkspaceName::parse("family").unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: personal_name.clone(),
        workspaces: std::collections::BTreeMap::from([
            (
                family_name,
                WorkspaceRecord {
                    workspace_id: WorkspaceId::new(),
                    root: family.clone(),
                    aliases: std::collections::BTreeSet::new(),
                    local_user_id: "family-user".to_owned(),
                    receiver_enabled: false,
                    env: serde_json::Map::new(),
                },
            ),
            (
                personal_name,
                WorkspaceRecord {
                    workspace_id: WorkspaceId::new(),
                    root: personal.clone(),
                    aliases: std::collections::BTreeSet::new(),
                    local_user_id: "personal-user".to_owned(),
                    receiver_enabled: false,
                    env: serde_json::Map::new(),
                },
            ),
        ]),
    };
    RegistryStore::from_path(env_path.clone())
        .replace(&registry)
        .unwrap();
    let registry_bytes = fs::read(&env_path).unwrap();

    let outcome = migrate_legacy(home.path(), &config_dir, &registry_bytes)
        .expect("upgrade valid registry portable policy");

    assert_eq!(fs::read(&env_path).unwrap(), registry_bytes);
    assert!(outcome.portable_setup_required);
    let personal_config: serde_json::Value =
        serde_json::from_slice(&fs::read(personal.join(".config/config.json")).unwrap()).unwrap();
    let family_config: serde_json::Value =
        serde_json::from_slice(&fs::read(family.join(".config/config.json")).unwrap()).unwrap();
    assert_eq!(personal_config["access_mode"], "unrestricted");
    assert_eq!(family_config["access_mode"], "workspace_only");
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
