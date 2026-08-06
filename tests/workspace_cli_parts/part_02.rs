
#[test]
fn workspace_attach_migrates_a_pointer_only_legacy_install_before_adding_shared() {
    let fixture = Fixture::new();
    let legacy = fixture.home.path().join("legacy-brain");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(
        fixture.config_home.path().join("brain-root"),
        format!("{}\n", legacy.display()),
    )
    .unwrap();
    let shared = fixture.home.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    let manifest = WorkspaceManifest::new(brain::workspace::WorkspaceId::new());
    manifest.write_new(&shared).unwrap();

    let output = fixture.run(&["workspace", "attach", path_arg(&shared)]);

    assert_success(&output);
    let registry = fixture.registry();
    assert_eq!(registry.default_workspace, name("legacy-brain"));
    assert_eq!(registry.workspaces.len(), 2);
    assert_eq!(registry.workspaces[&name("legacy-brain")].root, legacy);
    assert_eq!(registry.workspaces[&name("shared")].root, shared);
}

#[test]
fn ready_non_default_command_does_not_touch_default_workspace_migration_inputs() {
    let fixture = Fixture::new();
    let personal = fixture.home.path().join("personal");
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&personal)]));
    fixture.make_ready("personal");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    fixture.make_ready("family");

    let mut registry = fixture.registry();
    registry
        .workspaces
        .get_mut(&name("personal"))
        .expect("personal record")
        .env
        .insert(
            "markdown_to_pdf_path".to_owned(),
            serde_json::Value::String("/legacy/default/tool".to_owned()),
        );
    RegistryStore::from_path(fixture.registry_path())
        .replace(&registry)
        .expect("persist migratable default env");

    let default_config = personal.join(".config/config.json");
    std::fs::write(
        &default_config,
        b"{\n  \"markdown_to_pdf_path\": \"/legacy/default/tool\",\n  \"sentinel\": \"unchanged\"\n}\n",
    )
    .expect("default config fixture");
    let registry_before = std::fs::read(fixture.registry_path()).expect("registry bytes");
    let config_before = std::fs::read(&default_config).expect("default config bytes");

    let output = fixture.run(&["config", "get", "day_rollover_hour", "-b", "family"]);

    assert_success(&output);
    assert_eq!(
        std::fs::read(fixture.registry_path()).expect("registry after command"),
        registry_before,
        "ordinary selected-workspace bootstrap must not rerun legacy migration"
    );
    assert_eq!(
        std::fs::read(default_config).expect("default config after command"),
        config_before,
        "ordinary selected-workspace bootstrap must not read/migrate the default config"
    );
}

#[test]
fn leading_workspace_selector_reads_the_selected_portable_triage_flag() {
    let fixture = Fixture::new();
    let personal = fixture.home.path().join("personal");
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&personal)]));
    fixture.make_ready("personal");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    fixture.make_ready("family");
    std::fs::write(
        personal.join(".config/config.json"),
        b"{\"enable_triage_habits\":true}\n",
    )
    .unwrap();
    std::fs::write(
        family.join(".config/config.json"),
        b"{\"enable_triage_habits\":false}\n",
    )
    .unwrap();

    let default = fixture.run(&["config", "get", "enable_triage_habits"]);
    let selected = fixture.run(&["--brain", "family", "config", "get", "enable_triage_habits"]);

    assert_success(&default);
    assert_success(&selected);
    assert_eq!(String::from_utf8(default.stdout).unwrap().trim(), "true");
    assert_eq!(String::from_utf8(selected.stdout).unwrap().trim(), "false");
}

#[test]
fn workspace_create_treats_an_existing_default_root_as_legacy_install_evidence() {
    let fixture = Fixture::new();
    let legacy = fixture.home.path().join("brain");
    std::fs::create_dir_all(&legacy).unwrap();
    let family = fixture.home.path().join("family");

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&family)]);

    assert_success(&output);
    let registry = fixture.registry();
    assert_eq!(registry.default_workspace, name("brain"));
    assert_eq!(registry.workspaces.len(), 2);
    assert_eq!(registry.workspaces[&name("brain")].root, legacy);
    assert_eq!(registry.workspaces[&name("family")].root, family);
}
