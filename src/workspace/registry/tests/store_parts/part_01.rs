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
