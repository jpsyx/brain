use super::*;

#[test]
fn transaction_timeout_is_typed_and_names_the_lock_owner_and_remedy() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("env.json");
    let lock_path = directory.path().join(".env.json.transaction.lock");
    let holder = RegistryStore::from_path(path.clone());
    let error = holder
        .transaction(|_| -> Result<RegistryError, RegistryError> {
            let blocked = RegistryStore::from_path_with_lock_timeout(
                path,
                std::time::Duration::from_millis(20),
            );
            Ok(blocked
                .transaction::<(), RegistryError>(|_| {
                    panic!("held locks must block registry loading")
                })
                .unwrap_err())
        })
        .unwrap();

    assert!(matches!(
        error,
        RegistryError::LockTimeout {
            path: ref error_path,
            owner_pid: Some(owner_pid),
            waited_millis: 20,
        } if error_path == &lock_path && owner_pid == std::process::id()
    ));
    let message = error.to_string();
    assert!(message.contains("transaction lock"));
    assert!(message.contains("retry"));
    assert!(message.contains("operating system releases this lock"));
}

#[test]
fn zero_length_lock_artifact_recovers_after_owner_exits_without_dropping_guard() {
    const CHILD_ENV: &str = "BRAIN_REGISTRY_CRASHED_LOCK_CHILD";
    let Some(test_root) = std::env::var_os(CHILD_ENV).map(PathBuf::from) else {
        let directory = tempfile::tempdir().expect("tempdir");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "workspace::registry::tests::store::zero_length_lock_artifact_recovers_after_owner_exits_without_dropping_guard",
                "--nocapture",
            ])
            .env(CHILD_ENV, directory.path())
            .output()
            .expect("crashing lock-holder child");
        assert_eq!(output.status.code(), Some(86));
        let lock_path = directory.path().join(".env.json.transaction.lock");
        assert!(lock_path.is_file(), "the stable lock database remains");
        assert_eq!(
            fs::metadata(&lock_path).expect("lock metadata").len(),
            0,
            "locking must not depend on crash-sensitive schema initialization"
        );

        let store = RegistryStore::from_path_with_lock_timeout(
            directory.path().join("env.json"),
            std::time::Duration::from_millis(50),
        );
        store
            .transaction::<(), RegistryError>(|_| Ok(()))
            .expect("the operating system should release a crashed owner's lock");
        assert!(
            lock_path.is_file(),
            "guards never unlink the shared lock database"
        );
        return;
    };

    let store = RegistryStore::from_path(test_root.join("env.json"));
    let _ = store.transaction::<(), RegistryError>(|_| {
        std::process::exit(86);
    });
    unreachable!("process::exit never returns");
}

#[test]
fn store_load_preserves_unsupported_schema_error() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("env.json");
    let raw = format!(
        r#"{{
                "schema_version": 1,
                "default_workspace": "brain",
                "workspaces": {{
                    "brain": {{
                        "workspace_id": "{PERSONAL_ID}",
                        "root": "/workspaces/brain",
                        "local_user_id": "personal"
                    }}
                }}
            }}"#
    );
    fs::write(&path, raw).unwrap();

    assert_eq!(
        RegistryStore::load_from(&path),
        Err(RegistryError::UnsupportedSchemaVersion { found: 1 })
    );
}

#[test]
fn store_load_preserves_every_whole_registry_error_variant() {
    let mut empty = valid_registry_json();
    empty["workspaces"] = json!({});
    let mut missing_default = valid_registry_json();
    missing_default["default_workspace"] = json!("missing");
    let mut duplicate_selector = valid_registry_json();
    duplicate_selector["workspaces"]["brain"]["aliases"] = json!(["family"]);
    let mut duplicate_id = valid_registry_json();
    duplicate_id["workspaces"]["family"]["workspace_id"] = json!(PERSONAL_ID);
    let mut relative_root = valid_registry_json();
    relative_root["workspaces"]["family"]["root"] = json!("relative/family");
    let mut overlapping_root = valid_registry_json();
    overlapping_root["workspaces"]["family"]["root"] = json!("/workspaces/brain/family");

    let cases = [
        (empty, RegistryError::EmptyRegistry),
        (
            missing_default,
            RegistryError::MissingDefault {
                default_workspace: name("missing"),
            },
        ),
        (
            duplicate_selector,
            RegistryError::DuplicateSelector {
                selector: "family".to_owned(),
                first_workspace: name("family"),
                second_workspace: name("brain"),
            },
        ),
        (
            duplicate_id,
            RegistryError::DuplicateWorkspaceId {
                workspace_id: id(PERSONAL_ID),
            },
        ),
        (
            relative_root,
            RegistryError::RelativeRoot {
                canonical_name: name("family"),
                root: PathBuf::from("relative/family"),
            },
        ),
        (
            overlapping_root,
            RegistryError::OverlappingRoots {
                first: PathBuf::from("/workspaces/brain"),
                second: PathBuf::from("/workspaces/brain/family"),
            },
        ),
    ];

    let directory = tempfile::tempdir().expect("tempdir");
    for (index, (value, expected)) in cases.into_iter().enumerate() {
        let path = directory.path().join(format!("invalid-{index}.json"));
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(RegistryStore::load_from(&path), Err(expected));
    }
}

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

#[test]
fn read_error_identifies_the_operation_path_and_error_kind() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("missing.json");

    let error = RegistryStore::load_from(&path).unwrap_err();
    let display = error.to_string();

    assert!(matches!(
        error,
        RegistryError::Io {
            operation: RegistryOperation::ReadRegistry,
            path: ref error_path,
            related_path: None,
            kind: std::io::ErrorKind::NotFound,
            ..
        } if error_path == &path
    ));
    assert!(display.contains("read workspace registry"));
    assert!(display.contains(path.to_str().unwrap()));
}

#[test]
fn parse_error_identifies_the_operation_and_path() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("invalid.json");
    fs::write(&path, b"not json").unwrap();

    let error = RegistryStore::load_from(&path).unwrap_err();
    let display = error.to_string();

    assert!(matches!(
        error,
        RegistryError::Json {
            operation: RegistryOperation::ParseRegistry,
            path: ref error_path,
            ..
        } if error_path == &path
    ));
    assert!(display.contains("parse workspace registry JSON"));
    assert!(display.contains(path.to_str().unwrap()));
}

#[test]
fn generated_temporary_path_uses_the_destination_directory() {
    let destination = Path::new("/machine/config/env.json");
    let temporary = crate::workspace::registry::store::unique_temporary_path(destination);

    assert_eq!(temporary.parent(), destination.parent());
    assert_eq!(
        crate::workspace::registry::store::unique_temporary_path(Path::new("env.json")).parent(),
        Some(Path::new("."))
    );
}

#[test]
fn serialization_is_byte_for_byte_deterministic() {
    let directory = tempfile::tempdir().expect("tempdir");
    let first = directory.path().join("first.json");
    let second = directory.path().join("second.json");
    let registry = registry_with_brain_and_family();

    RegistryStore::save_atomic_to(&first, &registry).unwrap();
    RegistryStore::save_atomic_to(&second, &registry).unwrap();

    assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
}

#[cfg(unix)]
#[test]
fn atomic_save_creates_an_owner_only_destination() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("env.json");

    RegistryStore::save_atomic_to(&path, &registry_with_brain_and_family()).unwrap();

    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn bare_relative_save_uses_the_current_directory() {
    const CHILD_ENV: &str = "BRAIN_REGISTRY_RELATIVE_PATH_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let registry = registry_with_brain_and_family();
        RegistryStore::save_atomic_to(Path::new("env.json"), &registry)
            .expect("relative atomic save");
        assert_eq!(
            RegistryStore::load_from(Path::new("env.json")).unwrap(),
            registry
        );
        return;
    }

    let directory = tempfile::tempdir().expect("tempdir");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "workspace::registry::tests::store::bare_relative_save_uses_the_current_directory",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .current_dir(directory.path())
        .output()
        .expect("child test process");

    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(directory.path().join("env.json").is_file());
}

#[test]
fn bare_relative_path_parent_is_the_current_directory() {
    assert_eq!(
        crate::workspace::registry::store::parent_or_current_dir(Path::new("env.json")),
        Path::new(".")
    );
}

#[test]
fn store_update_returns_the_complete_removed_record() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("env.json");
    let mut registry = registry_with_brain_and_family();
    let original = registry.workspaces.get(&name("family")).unwrap().clone();
    RegistryStore::save_atomic_to(&path, &registry).unwrap();
    let store = RegistryStore::from_path(path.clone());

    let removed = store
        .update(&mut registry, |candidate| candidate.remove("family"))
        .unwrap();

    assert_eq!(removed, original);
    assert_eq!(removed.root, PathBuf::from("/workspaces/family"));
    assert!(!registry.workspaces.contains_key(&name("family")));
    assert_eq!(RegistryStore::load_from(&path).unwrap(), registry);
}
