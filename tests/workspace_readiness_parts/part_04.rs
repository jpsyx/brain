
#[test]
fn workspace_repair_rejects_blank_local_user_without_changing_registry_bytes() {
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
    assert!(create.status.success());
    let registry_path = config_home.path().join("brain/env.json");
    let before = std::fs::read(&registry_path).unwrap();

    let repair = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "workspace",
            "repair",
            "--local-user-id",
            "   ",
            "-b",
            "family",
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(!repair.status.success());
    assert!(String::from_utf8_lossy(&repair.stderr).contains("local user ID cannot be empty"));
    assert_eq!(std::fs::read(registry_path).unwrap(), before);
}

#[test]
fn interactive_bootstrap_repairs_then_continues_the_original_command() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    std::fs::create_dir_all(&root).unwrap();
    let canonical_name = WorkspaceName::parse("family").unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical_name.clone(),
        workspaces: std::collections::BTreeMap::from([(
            canonical_name,
            WorkspaceRecord {
                workspace_id,
                root: root.clone(),
                aliases: BTreeSet::new(),
                local_user_id: String::new(),
                receiver_enabled: false,
                env: Map::new(),
            },
        )]),
    };
    let store = RegistryStore::from_path(config_home.path().join("brain/env.json"));
    store.replace(&registry).unwrap();
    let mut cli = try_parse_from(["brain", "config", "list", "-b", "family"]).unwrap();
    let mut input = Cursor::new(b"Pablo\n\n".to_vec());
    let mut output = Vec::new();

    let outcome = bootstrap_with_io(
        &mut cli,
        store,
        home.path(),
        home.path(),
        InteractionMode::Interactive,
        &mut input,
        &mut output,
    )
    .unwrap();

    let BootstrapContext::Ready(context) = outcome else {
        panic!("ordinary config command must continue with a ready context");
    };
    assert_eq!(context.workspace.local_user_id(), "pablo");
    assert_eq!(context.workspace.id(), workspace_id);
    assert!(WorkspaceManifest::path(&root).is_file());
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "Your display name: User ID [pablo]: "
    );
}

#[test]
fn interactive_first_user_setup_uses_display_name_and_accepts_the_proposed_id() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    std::fs::create_dir_all(&root).unwrap();
    let canonical_name = WorkspaceName::parse("family").unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    WorkspaceManifest::new(workspace_id)
        .write_new(&root)
        .unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical_name.clone(),
        workspaces: std::collections::BTreeMap::from([(
            canonical_name,
            WorkspaceRecord {
                workspace_id,
                root,
                aliases: BTreeSet::new(),
                local_user_id: String::new(),
                receiver_enabled: false,
                env: Map::new(),
            },
        )]),
    };
    let store = RegistryStore::from_path(config_home.path().join("brain/env.json"));
    store.replace(&registry).unwrap();
    let mut cli = try_parse_from(["brain", "config", "list", "-b", "family"]).unwrap();
    let mut input = Cursor::new(b"Alex Smith\n\n".to_vec());
    let mut output = Vec::new();

    let outcome = bootstrap_with_io(
        &mut cli,
        store,
        home.path(),
        home.path(),
        InteractionMode::Interactive,
        &mut input,
        &mut output,
    )
    .unwrap();

    let BootstrapContext::Ready(context) = outcome else {
        panic!("first user setup must continue the command");
    };
    assert_eq!(context.workspace.local_user_id(), "alex-smith");
    let users = UsersStore::load(&context.workspace).unwrap();
    let user = users
        .user(&brain::users::UserId::parse("alex-smith").unwrap())
        .unwrap();
    assert_eq!(user.name, "Alex Smith");
    assert!(user.phones.is_empty());
    assert!(user.emails.is_empty());
    assert_eq!(
        String::from_utf8(output).unwrap(),
        "Your display name: User ID [alex-smith]: "
    );
}

#[test]
fn first_user_setup_asks_for_contacts_only_for_configured_receiver_channels() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("family");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    WorkspaceManifest::new(workspace_id)
        .write_new(&root)
        .unwrap();
    std::fs::write(
        root.join(".config/config.json"),
        br#"{"allowed_sms_senders":"+12125550100","allowed_email_senders":"alex@example.com,relative@example.com","response_email":"alex@example.com"}"#,
    )
    .unwrap();
    let canonical_name = WorkspaceName::parse("family").unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical_name.clone(),
        workspaces: std::collections::BTreeMap::from([(
            canonical_name,
            WorkspaceRecord {
                workspace_id,
                root,
                aliases: BTreeSet::new(),
                local_user_id: String::new(),
                receiver_enabled: true,
                env: Map::new(),
            },
        )]),
    };
    let store = RegistryStore::from_path(config_home.path().join("brain/env.json"));
    store.replace(&registry).unwrap();
    let mut cli = try_parse_from(["brain", "config", "list", "-b", "family"]).unwrap();
    let mut input = Cursor::new(b"Alex Smith\n\n\n\n".to_vec());
    let mut output = Vec::new();

    let outcome = bootstrap_with_io(
        &mut cli,
        store,
        home.path(),
        home.path(),
        InteractionMode::Interactive,
        &mut input,
        &mut output,
    )
    .unwrap();

    let BootstrapContext::Ready(context) = outcome else {
        panic!("configured receiver setup must continue the command");
    };
    let users = UsersStore::load(&context.workspace).unwrap();
    let user = &users.users[0];
    assert_eq!(user.phones[0].value, "+12125550100");
    assert_eq!(user.emails[0].value, "alex@example.com");
    assert_eq!(user.response_email.as_deref(), Some("alex@example.com"));
    let prompts = String::from_utf8(output).unwrap();
    assert!(prompts.contains("Phone [+12125550100]:"));
    assert!(prompts.contains("Email [alex@example.com]:"));
    assert!(
        !user
            .emails
            .iter()
            .any(|email| email.value == "relative@example.com")
    );
}
