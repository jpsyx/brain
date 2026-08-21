
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
fn migration_adopts_an_existing_portable_manifest_without_changing_its_identity() {
    let (home, _config_home, config_dir) = fixture_dirs();
    let env_path = config_dir.join("env.json");
    let root = home.path().join("brain");
    fs::create_dir_all(&root).unwrap();
    let legacy = br#"{"root":"~/brain","custom":"keep"}"#;
    fs::write(&env_path, legacy).unwrap();
    let portable =
        WorkspaceManifest::new(WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap());
    portable.write_new(&root).unwrap();
    let manifest_path = WorkspaceManifest::path(&root);
    let original_bytes = fs::read(&manifest_path).unwrap();
    let original_ingress = portable.receiver_ingress_id();

    let outcome = migrate_legacy(home.path(), &config_dir, legacy).unwrap();

    let selected = outcome.registry.select(None).unwrap();
    assert_eq!(selected.record().workspace_id, portable.workspace_id());
    assert_eq!(fs::read(&manifest_path).unwrap(), original_bytes);
    assert_eq!(
        WorkspaceManifest::load(&root, env!("CARGO_PKG_VERSION"))
            .unwrap()
            .receiver_ingress_id(),
        original_ingress
    );
}
