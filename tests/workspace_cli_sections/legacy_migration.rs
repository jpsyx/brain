#[test]
fn first_workspace_create_uses_root_basename_and_becomes_default() {
    let fixture = Fixture::new();
    let root = fixture.home.path().join("Family");

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&root)]);

    assert_success(&output);
    assert!(root.is_dir());
    let registry = fixture.registry();
    assert_eq!(registry.default_workspace, name("family"));
    assert_eq!(registry.workspaces.len(), 1);
    assert_eq!(registry.workspaces[&name("family")].root, root);
    let manifest = WorkspaceManifest::load(&root, env!("CARGO_PKG_VERSION"))
        .expect("created workspace manifest");
    assert_eq!(
        manifest.workspace_id(),
        registry.workspaces[&name("family")].workspace_id
    );
}

#[test]
fn workspace_create_migrates_a_flat_env_before_adding_the_requested_workspace() {
    let fixture = Fixture::new();
    let machine_config = fixture.config_home.path().join("brain");
    std::fs::create_dir_all(&machine_config).unwrap();
    std::fs::write(
        machine_config.join("env.json"),
        br#"{"root":"~/brain","claude_cmd":"claude --legacy"}"#,
    )
    .unwrap();
    let family = fixture.home.path().join("family");

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&family)]);

    assert_success(&output);
    let registry = fixture.registry();
    assert_eq!(registry.default_workspace, name("brain"));
    assert_eq!(registry.workspaces.len(), 2);
    assert_eq!(
        registry.workspaces[&name("brain")].root,
        fixture.home.path().join("brain")
    );
    assert_eq!(registry.workspaces[&name("family")].root, family);
}

#[test]
fn workspace_attach_migrates_a_flat_env_before_adding_the_requested_workspace() {
    let fixture = Fixture::new();
    let machine_config = fixture.config_home.path().join("brain");
    std::fs::create_dir_all(&machine_config).unwrap();
    std::fs::write(machine_config.join("env.json"), br#"{"root":"~/brain"}"#).unwrap();
    let shared = fixture.home.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    let manifest = WorkspaceManifest::new(brain::workspace::WorkspaceId::new());
    manifest.write_new(&shared).unwrap();

    let output = fixture.run(&["workspace", "attach", path_arg(&shared)]);

    assert_success(&output);
    let registry = fixture.registry();
    assert_eq!(registry.default_workspace, name("brain"));
    assert_eq!(registry.workspaces.len(), 2);
    assert_eq!(
        registry.workspaces[&name("shared")].workspace_id,
        manifest.workspace_id()
    );
}

#[test]
fn workspace_create_migrates_a_pointer_only_legacy_install_before_adding_family() {
    let fixture = Fixture::new();
    let legacy = fixture.home.path().join("legacy-brain");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(
        fixture.config_home.path().join("brain-root"),
        format!("{}\n", legacy.display()),
    )
    .unwrap();
    let family = fixture.home.path().join("family");

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&family)]);

    assert_success(&output);
    let registry = fixture.registry();
    assert_eq!(registry.default_workspace, name("legacy-brain"));
    assert_eq!(registry.workspaces.len(), 2);
    assert_eq!(registry.workspaces[&name("legacy-brain")].root, legacy);
    assert_eq!(registry.workspaces[&name("family")].root, family);
}
