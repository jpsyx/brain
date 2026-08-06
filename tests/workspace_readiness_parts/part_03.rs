
#[test]
fn readiness_prompts_interactively_and_errors_actionably_when_headless() {
    let record = WorkspaceRecord {
        workspace_id: WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        root: PathBuf::from("/brains/family"),
        aliases: BTreeSet::new(),
        local_user_id: String::new(),
        receiver_enabled: false,
        env: Map::new(),
    };
    let missing = ManifestError::Io {
        operation: "read workspace manifest",
        path: PathBuf::from("/brains/family/.config/workspace.json"),
        kind: std::io::ErrorKind::NotFound,
        message: "not found".to_owned(),
    };

    assert_eq!(
        readiness_action(
            &WorkspaceName::parse("family").unwrap(),
            &record,
            Err(missing.clone()),
            InteractionMode::Interactive,
        )
        .unwrap(),
        ReadinessAction::Prompt(vec![ReadinessField::Manifest, ReadinessField::LocalUserId])
    );

    let error = readiness_action(
        &WorkspaceName::parse("family").unwrap(),
        &record,
        Err(missing),
        InteractionMode::NonInteractive,
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("brain workspace repair -b family --manifest"));
    assert!(message.contains("brain user local <USER_ID> -b family"));
}

#[test]
fn readiness_rejects_a_manifest_for_a_different_registry_uuid() {
    let record = WorkspaceRecord {
        workspace_id: WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
        root: PathBuf::from("/brains/family"),
        aliases: BTreeSet::new(),
        local_user_id: "pablo".to_owned(),
        receiver_enabled: false,
        env: Map::new(),
    };
    let body = br#"{"schema_version":1,"workspace_id":"e806258e-491a-436d-9db4-a5ca9903e0d4","receiver_ingress_id":"57b162df-983a-45c3-ac7e-bad94eb27a99","minimum_brain_version":"0.16.0"}"#;
    let manifest = WorkspaceManifest::parse(body, "0.16.0").unwrap();

    let error = readiness_action(
        &WorkspaceName::parse("family").unwrap(),
        &record,
        Ok(manifest),
        InteractionMode::NonInteractive,
    )
    .unwrap_err();

    assert!(error.to_string().contains("does not match registry UUID"));
}

#[test]
fn workspace_repair_creates_the_matching_manifest_and_sets_local_user() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    let create = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["workspace", "create", "--root", root.to_str().unwrap()])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );
    std::fs::remove_file(WorkspaceManifest::path(&root)).unwrap();

    let repair = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "workspace",
            "repair",
            "--manifest",
            "--local-user-id",
            "pablo",
            "-b",
            "family",
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        repair.status.success(),
        "{}",
        String::from_utf8_lossy(&repair.stderr)
    );

    let registry = RegistryStore::load_from(&config_home.path().join("brain/env.json")).unwrap();
    let selected = registry.select(Some("family")).unwrap();
    assert_eq!(selected.record().local_user_id, "pablo");
    let manifest = WorkspaceManifest::load(&root, env!("CARGO_PKG_VERSION")).unwrap();
    assert_eq!(manifest.workspace_id(), selected.record().workspace_id);
}

#[test]
fn first_create_is_registry_only_and_the_next_headless_command_names_the_exact_repair() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    let create = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["workspace", "create", "--root", root.to_str().unwrap()])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );

    let registry_path = config_home.path().join("brain/env.json");
    let registry = RegistryStore::load_from(&registry_path).unwrap();
    assert!(
        registry
            .select(None)
            .unwrap()
            .record()
            .local_user_id
            .is_empty()
    );
    assert!(WorkspaceManifest::path(&root).is_file());

    let blocked = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["config", "list"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!blocked.status.success());
    let stderr = String::from_utf8(blocked.stderr).unwrap();
    assert!(stderr.contains("brain user add -b family --id <USER_ID> --name <DISPLAY_NAME>"));
    assert!(stderr.contains("brain user local <USER_ID> -b family"));
    assert!(
        !stderr.contains("--manifest"),
        "create already wrote the manifest: {stderr}"
    );

    let add = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "-b", "family", "user", "add", "--id", "pablo", "--name", "Pablo",
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let local = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["user", "local", "pablo", "-b", "family"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        local.status.success(),
        "{}",
        String::from_utf8_lossy(&local.stderr)
    );

    let continued = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["config", "list", "-b", "family"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        continued.status.success(),
        "{}",
        String::from_utf8_lossy(&continued.stderr)
    );
}
