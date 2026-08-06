
#[test]
fn response_email_alone_does_not_enable_or_prompt_for_an_email_identity() {
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
        br#"{"response_email":"alex@example.com"}"#,
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
        panic!("response-only setup must continue without an email prompt");
    };
    let users = UsersStore::load(&context.workspace).unwrap();
    assert!(users.users[0].emails.is_empty());
    assert!(users.users[0].response_email.is_none());
    assert!(!String::from_utf8(output).unwrap().contains("Email"));
}

#[test]
fn manifest_parsing_is_strict_and_checks_compatibility() {
    let workspace_id = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";
    let ingress_id = "e806258e-491a-436d-9db4-a5ca9903e0d4";
    let valid = format!(
        r#"{{"schema_version":1,"workspace_id":"{workspace_id}","receiver_ingress_id":"{ingress_id}","minimum_brain_version":"0.16.0"}}"#
    );

    let manifest = WorkspaceManifest::parse(valid.as_bytes(), "0.16.0").expect("valid manifest");
    assert_eq!(
        manifest.workspace_id(),
        WorkspaceId::parse(workspace_id).unwrap()
    );
    assert_eq!(
        manifest.receiver_ingress_id(),
        WorkspaceId::parse(ingress_id).unwrap()
    );

    let unknown = valid.replace('}', ",\"unexpected\":true}");
    assert!(matches!(
        WorkspaceManifest::parse(unknown.as_bytes(), "0.16.0"),
        Err(ManifestError::InvalidJson { .. })
    ));
    let unsupported = valid.replace("\"schema_version\":1", "\"schema_version\":2");
    assert!(matches!(
        WorkspaceManifest::parse(unsupported.as_bytes(), "0.16.0"),
        Err(ManifestError::UnsupportedSchema {
            found: 2,
            supported: 1
        })
    ));
    let incompatible = valid.replace("0.16.0", "0.17.0");
    assert!(matches!(
        WorkspaceManifest::parse(incompatible.as_bytes(), "0.16.0"),
        Err(ManifestError::IncompatibleBrainVersion { .. })
    ));
}

#[test]
fn writing_a_new_manifest_is_create_only_and_round_trips() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace_id = WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap();
    let manifest = WorkspaceManifest::new(workspace_id);

    manifest.write_new(fixture.path()).expect("first write");
    let original_bytes = std::fs::read(WorkspaceManifest::path(fixture.path())).unwrap();
    let loaded = WorkspaceManifest::load(fixture.path(), env!("CARGO_PKG_VERSION")).unwrap();
    assert_eq!(loaded, manifest);
    let replacement =
        WorkspaceManifest::new(WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap());
    let error = replacement.write_new(fixture.path()).unwrap_err();
    assert!(matches!(
        error,
        ManifestError::Io {
            kind: std::io::ErrorKind::AlreadyExists,
            ..
        }
    ));
    assert_eq!(
        std::fs::read(WorkspaceManifest::path(fixture.path())).unwrap(),
        original_bytes
    );
}

#[test]
fn parsed_routes_map_to_their_explicit_invocations() {
    let cases = [
        (vec!["brain"], Invocation::Tui),
        (
            vec!["brain", "workspace", "create", "--root", "/tmp/new"],
            Invocation::WorkspaceCreate,
        ),
        (
            vec!["brain", "workspace", "attach", "/tmp/existing"],
            Invocation::WorkspaceAttach,
        ),
        (
            vec!["brain", "workspace", "remove", "old"],
            Invocation::WorkspaceRemove,
        ),
        (
            vec![
                "brain",
                "workspace",
                "repair",
                "--manifest",
                "--local-user-id",
                "pablo",
            ],
            Invocation::WorkspaceRepair,
        ),
        (
            vec!["brain", "workspace", "list"],
            Invocation::WorkspaceList,
        ),
        (vec!["brain", "user", "list"], Invocation::User),
        (vec!["brain", "config"], Invocation::Config),
        (vec!["brain", "env"], Invocation::Env),
        (vec!["brain", "sync"], Invocation::Sync),
        (vec!["brain", "sync", "status"], Invocation::SyncStatus),
        (vec!["brain", "check"], Invocation::Check),
        (vec!["brain", "personalize"], Invocation::Personalize),
        (vec!["brain", "skills"], Invocation::Skills),
        (vec!["brain", "server", "status"], Invocation::ServerStatus),
        (vec!["brain", "server", "logs"], Invocation::Server),
        (
            vec![
                "brain",
                "server",
                "run",
                "--generation",
                "57b162df-983a-45c3-ac7e-bad94eb27a99",
                "--port",
                "8765",
            ],
            Invocation::InternalServer,
        ),
        (
            vec!["brain", "receiver", "status"],
            Invocation::ReceiverStatus,
        ),
        (vec!["brain", "receiver", "start"], Invocation::Receiver),
        (vec!["brain", "habits"], Invocation::Habits),
        (vec!["brain", "reindex"], Invocation::Reindex),
        (
            vec!["brain", "tasks", "today", "--no-tui"],
            Invocation::Tasks,
        ),
        (vec!["brain", "tasks", "doctor"], Invocation::TasksDoctor),
        (vec!["brain", "version"], Invocation::Version),
    ];

    for (argv, expected) in cases {
        let cli = try_parse_from(argv.clone()).expect("route parses");
        assert_eq!(invocation_for(&cli), expected, "{argv:?}");
    }
}
