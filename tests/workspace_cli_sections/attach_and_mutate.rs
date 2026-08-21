
#[test]
fn workspace_attach_registers_an_existing_root_without_changing_its_contents() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    let shared = fixture.home.path().join("shared");
    std::fs::create_dir(&shared).expect("existing shared root");
    let attached_manifest = WorkspaceManifest::new(brain::workspace::WorkspaceId::new());
    attached_manifest
        .write_new(&shared)
        .expect("portable manifest");
    let manifest_bytes = std::fs::read(WorkspaceManifest::path(&shared)).unwrap();
    let sentinel = shared.join("keep.txt");
    std::fs::write(&sentinel, "untouched").expect("sentinel");

    let output = fixture.run(&["workspace", "attach", path_arg(&shared)]);

    assert_success(&output);
    let registry = fixture.registry();
    assert_eq!(registry.workspaces[&name("shared")].root, shared);
    assert_eq!(
        registry.workspaces[&name("shared")].workspace_id,
        attached_manifest.workspace_id()
    );
    assert_eq!(
        WorkspaceManifest::load(&shared, env!("CARGO_PKG_VERSION"))
            .unwrap()
            .receiver_ingress_id(),
        attached_manifest.receiver_ingress_id()
    );
    assert_eq!(
        std::fs::read(WorkspaceManifest::path(&shared)).unwrap(),
        manifest_bytes
    );
    assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "untouched");
}

#[test]
fn workspace_attach_rejects_invalid_or_colliding_manifests_without_mutation() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    let registry_bytes = std::fs::read(fixture.registry_path()).unwrap();
    let family_id = fixture.registry().workspaces[&name("family")].workspace_id;

    let invalid = fixture.home.path().join("invalid");
    std::fs::create_dir_all(invalid.join(".config")).unwrap();
    std::fs::write(
        WorkspaceManifest::path(&invalid),
        br#"{"schema_version":1,"unexpected":true}"#,
    )
    .unwrap();
    let invalid_output = fixture.run(&["workspace", "attach", path_arg(&invalid)]);
    assert_failure_contains(
        &invalid_output,
        &["Workspace error:", "invalid workspace manifest"],
    );
    assert_eq!(
        std::fs::read(fixture.registry_path()).unwrap(),
        registry_bytes
    );

    let colliding = fixture.home.path().join("colliding");
    std::fs::create_dir(&colliding).unwrap();
    WorkspaceManifest::new(family_id)
        .write_new(&colliding)
        .unwrap();
    let sentinel = colliding.join("keep.txt");
    std::fs::write(&sentinel, "preserved").unwrap();
    let collision_output = fixture.run(&["workspace", "attach", path_arg(&colliding)]);
    assert_failure_contains(
        &collision_output,
        &["Workspace error:", "workspace ID", "not unique"],
    );
    assert_eq!(
        std::fs::read(fixture.registry_path()).unwrap(),
        registry_bytes
    );
    assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "preserved");
}

#[cfg(unix)]
#[test]
fn workspace_repair_persistence_failure_preserves_the_new_manifest() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    std::fs::remove_file(WorkspaceManifest::path(&family)).unwrap();
    let registry_bytes = std::fs::read(fixture.registry_path()).unwrap();
    let registry_dir = fixture.registry_path().parent().unwrap().to_path_buf();
    let read_only = ReadOnlyDir::new(&registry_dir);

    let output = fixture.run(&[
        "workspace",
        "repair",
        "--manifest",
        "--local-user-id",
        "pablo",
        "-b",
        "family",
    ]);

    drop(read_only);
    assert_failure_contains(
        &output,
        &["Workspace error:", "create temporary workspace registry"],
    );
    assert_eq!(
        std::fs::read(fixture.registry_path()).unwrap(),
        registry_bytes
    );
    let manifest = WorkspaceManifest::load(&family, env!("CARGO_PKG_VERSION")).unwrap();
    assert_eq!(
        manifest.workspace_id(),
        fixture.registry().workspaces[&name("family")].workspace_id
    );
}

#[test]
fn alias_rename_and_default_mutations_preserve_the_complete_workspace_record() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    let work = fixture.home.path().join("work");
    for root in [&family, &work] {
        assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(root)]));
    }
    fixture.make_ready("family");
    let mut registry = fixture.registry();
    let work_record = registry.workspaces.get_mut(&name("work")).unwrap();
    work_record.local_user_id = "person-7".to_owned();
    work_record.receiver_enabled = true;
    work_record
        .env
        .insert("custom".to_owned(), serde_json::json!({"nested": 42}));
    RegistryStore::from_path(fixture.registry_path())
        .replace(&registry)
        .unwrap();
    let original = registry.workspaces[&name("work")].clone();
    let manifest_path = WorkspaceManifest::path(&work);
    let original_manifest_bytes = std::fs::read(&manifest_path).unwrap();
    let original_ingress = WorkspaceManifest::load(&work, env!("CARGO_PKG_VERSION"))
        .unwrap()
        .receiver_ingress_id();

    assert_success(&fixture.run(&["workspace", "alias", "add", "work", "job"]));
    assert_success(&fixture.run(&["workspace", "rename", "job", "office"]));
    assert_success(&fixture.run(&["workspace", "alias", "remove", "office", "job"]));
    assert_success(&fixture.run(&["workspace", "alias", "add", "office", "workplace"]));
    assert_success(&fixture.run(&["workspace", "default", "workplace"]));

    let registry = fixture.registry();
    let renamed = &registry.workspaces[&name("office")];
    assert_eq!(registry.default_workspace, name("office"));
    assert_eq!(renamed.workspace_id, original.workspace_id);
    assert_eq!(renamed.root, original.root);
    assert_eq!(renamed.local_user_id, original.local_user_id);
    assert_eq!(renamed.receiver_enabled, original.receiver_enabled);
    assert_eq!(renamed.env, original.env);
    assert_eq!(
        renamed.aliases,
        std::iter::once(name("workplace")).collect()
    );
    assert_eq!(
        std::fs::read(&manifest_path).unwrap(),
        original_manifest_bytes
    );
    assert_eq!(
        WorkspaceManifest::load(&work, env!("CARGO_PKG_VERSION"))
            .unwrap()
            .receiver_ingress_id(),
        original_ingress
    );
}
