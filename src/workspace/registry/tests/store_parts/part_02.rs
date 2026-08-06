
#[test]
fn failed_validation_keeps_memory_and_original_file_bytes_unchanged() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("env.json");
    let mut registry = registry_with_brain_and_family();
    RegistryStore::save_atomic_to(&path, &registry).expect("initial save");
    let original_registry = registry.clone();
    let original_bytes = fs::read(&path).expect("original bytes");
    let store = RegistryStore::from_path(path.clone());

    let error = store
        .update(&mut registry, |candidate| {
            candidate.schema_version = 99;
            Ok(())
        })
        .unwrap_err();

    assert_eq!(error, RegistryError::UnsupportedSchemaVersion { found: 99 });
    assert_eq!(registry, original_registry);
    assert_eq!(fs::read(path).unwrap(), original_bytes);
}

#[test]
fn persistence_failure_keeps_memory_and_original_file_bytes_unchanged() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("env.json");
    let blocked_temporary = directory.path().join("blocked-temporary");
    fs::create_dir(&blocked_temporary).expect("blocking directory");
    let mut registry = registry_with_brain_and_family();
    RegistryStore::save_atomic_to(&path, &registry).expect("initial save");
    let original_registry = registry.clone();
    let original_bytes = fs::read(&path).expect("original bytes");
    let store = RegistryStore::from_path_with_temporary(path.clone(), blocked_temporary.clone());

    let error = store
        .update(&mut registry, |candidate| candidate.set_default("family"))
        .unwrap_err();

    let display = error.to_string();
    assert!(matches!(
        error,
        RegistryError::Io {
            operation: RegistryOperation::CreateTemporary,
            path: ref error_path,
            related_path: Some(ref related_path),
            kind: std::io::ErrorKind::AlreadyExists,
            ..
        } if error_path == &blocked_temporary && related_path == &path
    ));
    assert!(display.contains("create temporary workspace registry"));
    assert!(display.contains(blocked_temporary.to_str().unwrap()));
    assert!(display.contains(path.to_str().unwrap()));
    assert_eq!(registry, original_registry);
    assert_eq!(fs::read(path).unwrap(), original_bytes);
}

#[test]
fn post_create_rename_failure_cleans_up_the_temporary_file() {
    let directory = tempfile::tempdir().expect("tempdir");
    let destination = directory.path().join("env.json");
    let temporary = directory.path().join("injected.tmp");
    let registry = registry_with_brain_and_family();
    RegistryStore::save_atomic_to(&destination, &registry).unwrap();
    let store = RegistryStore::from_path_with_temporary(destination.clone(), temporary.clone());

    let error = store
        .transaction(|transaction| {
            let mut latest = transaction.load()?;
            fs::remove_file(&destination).unwrap();
            fs::create_dir(&destination).unwrap();
            fs::write(destination.join("keep"), b"keep").unwrap();
            transaction.update(&mut latest, |candidate| candidate.set_default("family"))
        })
        .unwrap_err();
    let display = error.to_string();

    assert!(matches!(
        error,
        RegistryError::Io {
            operation: RegistryOperation::ReplaceRegistry,
            path: ref error_path,
            related_path: Some(ref related_path),
            ..
        } if error_path == &destination && related_path == &temporary
    ));
    assert!(display.contains("replace workspace registry"));
    assert!(display.contains(destination.to_str().unwrap()));
    assert!(display.contains(temporary.to_str().unwrap()));
    assert!(!temporary.exists());
    assert_eq!(fs::read(destination.join("keep")).unwrap(), b"keep");
}

#[test]
fn valid_atomic_save_and_load_round_trip() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("env.json");
    let registry = registry_with_brain_and_family();

    RegistryStore::save_atomic_to(&path, &registry).expect("atomic save");

    assert_eq!(RegistryStore::load_from(&path).unwrap(), registry);
}
