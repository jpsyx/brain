
#[test]
fn several_portable_users_still_require_an_explicit_local_choice() {
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let manifest = WorkspaceManifest::new(workspace_id);
    let name = WorkspaceName::parse("family").unwrap();
    let record = record_without_local_user(PathBuf::from("/brains/family"), workspace_id);

    assert_eq!(
        readiness_action_with_users(
            &name,
            &record,
            Ok(manifest.clone()),
            Ok(users_named(&["pablo", "sam"])),
            InteractionMode::Interactive,
        )
        .unwrap(),
        ReadinessAction::Prompt(vec![ReadinessField::LocalUserId])
    );

    assert!(matches!(
        readiness_action_with_users(
            &name,
            &record,
            Ok(manifest),
            Ok(users_named(&["pablo", "sam"])),
            InteractionMode::NonInteractive,
        )
        .unwrap_err(),
        brain::workspace::ReadinessError::Incomplete { .. }
    ));
}

#[test]
fn an_explicitly_set_but_unknown_local_user_is_never_auto_adopted() {
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let manifest = WorkspaceManifest::new(workspace_id);
    let name = WorkspaceName::parse("brain").unwrap();
    let mut record = record_without_local_user(PathBuf::from("/brains/brain"), workspace_id);
    "ghost".clone_into(&mut record.local_user_id);

    let error = readiness_action_with_users(
        &name,
        &record,
        Ok(manifest),
        Ok(users_named(&["pablo"])),
        InteractionMode::NonInteractive,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        brain::workspace::ReadinessError::InvalidLocalUser { .. }
    ));
}

#[test]
fn headless_command_self_heals_a_sole_user_workspace_and_continues() {
    let home = tempfile::tempdir().unwrap();
    let config_home = tempfile::tempdir().unwrap();
    let root = home.path().join("brain");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    let canonical_name = WorkspaceName::parse("brain").unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    WorkspaceManifest::new(workspace_id)
        .write_new(&root)
        .unwrap();
    UsersStore::save_to(&root.join(".config/users.json"), &users_named(&["pablo"])).unwrap();
    let registry = MachineRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        default_workspace: canonical_name.clone(),
        workspaces: std::collections::BTreeMap::from([(
            canonical_name,
            record_without_local_user(root, workspace_id),
        )]),
        env: serde_json::Map::new(),
    };
    let registry_path = config_home.path().join("brain/env.json");
    let store = RegistryStore::from_path(registry_path.clone());
    store.replace(&registry).unwrap();
    let mut cli = try_parse_from(["brain", "config", "list", "-b", "brain"]).unwrap();

    let outcome = bootstrap_with_io(
        &mut cli,
        store,
        home.path(),
        home.path(),
        InteractionMode::NonInteractive,
        &mut std::io::empty(),
        &mut std::io::sink(),
    )
    .unwrap();

    let BootstrapContext::Ready(context) = outcome else {
        panic!("a sole-user workspace must self-heal and continue headlessly");
    };
    assert_eq!(context.workspace.local_user_id(), "pablo");
    let healed = RegistryStore::load_from(&registry_path).unwrap();
    assert_eq!(
        healed.select(Some("brain")).unwrap().record().local_user_id,
        "pablo"
    );
}

#[test]
fn every_invocation_has_an_explicit_bootstrap_policy() {
    let cases = [
        (Invocation::Version, BootstrapPolicy::None),
        (Invocation::Help, BootstrapPolicy::None),
        (Invocation::AgentHook, BootstrapPolicy::InternalNoPrompt),
        (
            Invocation::InternalServer,
            BootstrapPolicy::InternalNoPrompt,
        ),
        (Invocation::WorkspaceCreate, BootstrapPolicy::RegistryOnly),
        (Invocation::WorkspaceAttach, BootstrapPolicy::RegistryOnly),
        (Invocation::WorkspaceRemove, BootstrapPolicy::RegistryOnly),
        (Invocation::WorkspaceRepair, BootstrapPolicy::RegistryOnly),
        (Invocation::User, BootstrapPolicy::RegistryOnly),
        (
            Invocation::WorkspaceList,
            BootstrapPolicy::ReadOnlyWorkspace,
        ),
        (Invocation::WorkspaceRename, BootstrapPolicy::ReadyWorkspace),
        (Invocation::WorkspaceAlias, BootstrapPolicy::ReadyWorkspace),
        (
            Invocation::WorkspaceDefault,
            BootstrapPolicy::ReadyWorkspace,
        ),
        (Invocation::Config, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Env, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Sync, BootstrapPolicy::ReadyWorkspace),
        (Invocation::SyncStatus, BootstrapPolicy::ReadOnlyWorkspace),
        (Invocation::Check, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Persona, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Skills, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Server, BootstrapPolicy::None),
        (Invocation::ServerStatus, BootstrapPolicy::None),
        (Invocation::Receiver, BootstrapPolicy::ReadyWorkspace),
        (
            Invocation::ReceiverStatus,
            BootstrapPolicy::ReadOnlyWorkspace,
        ),
        (Invocation::Habits, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Reindex, BootstrapPolicy::ReadyWorkspace),
        (Invocation::Tasks, BootstrapPolicy::ReadyWorkspace),
        (Invocation::TasksDoctor, BootstrapPolicy::ReadOnlyWorkspace),
        (Invocation::Tui, BootstrapPolicy::ReadyWorkspace),
    ];

    for (invocation, expected) in cases {
        assert_eq!(bootstrap_policy(invocation), expected, "{invocation:?}");
    }
}
