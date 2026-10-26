
#[test]
fn later_workspace_create_preserves_the_existing_default() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    let work = fixture.home.path().join("work");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&work)]);

    assert_success(&output);
    let registry = fixture.registry();
    assert_eq!(registry.default_workspace, name("family"));
    assert_eq!(registry.workspaces.len(), 2);
    assert_eq!(registry.workspaces[&name("work")].root, work);
}

#[cfg(unix)]
#[test]
fn first_create_persistence_failure_preserves_its_new_root_chain_for_manual_cleanup() {
    let fixture = Fixture::new();
    let registry_dir = fixture.registry_path().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&registry_dir).expect("registry directory");
    std::fs::File::create(registry_dir.join(".env.json.transaction.lock"))
        .expect("zero-length transaction lock database");
    std::fs::write(
        registry_dir.join(".env.json.transaction.lock.owner"),
        std::process::id().to_string(),
    )
    .expect("stable transaction lock owner file");
    let root_parent = fixture.home.path().join("created-only-by-command");
    let root = root_parent.join("nested/family");
    let read_only = ReadOnlyDir::new(&registry_dir);

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&root)]);

    drop(read_only);
    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "create temporary workspace registry",
            "automatic cleanup was skipped",
            "deepest first",
        ],
    );
    assert!(!fixture.registry_path().exists());
    assert!(root_parent.is_dir());
    assert!(root.is_dir());
    assert!(
        WorkspaceManifest::path(&root).is_file(),
        "a valid manifest must survive registry persistence failure"
    );
}

#[cfg(unix)]
#[test]
fn later_create_persistence_failure_preserves_registry_bytes_and_new_root_chain() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    let registry_bytes = std::fs::read(fixture.registry_path()).expect("registry bytes");
    let registry_dir = fixture.registry_path().parent().unwrap().to_path_buf();
    let root_parent = fixture.home.path().join("created-only-by-command");
    let root = root_parent.join("nested/work");
    let read_only = ReadOnlyDir::new(&registry_dir);

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&root)]);

    drop(read_only);
    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "create temporary workspace registry",
            "automatic cleanup was skipped",
            "deepest first",
        ],
    );
    assert_eq!(
        std::fs::read(fixture.registry_path()).expect("registry bytes after failure"),
        registry_bytes
    );
    assert!(root_parent.is_dir());
    assert!(root.is_dir());
    assert!(
        WorkspaceManifest::path(&root).is_file(),
        "a valid manifest must survive registry persistence failure"
    );
}
