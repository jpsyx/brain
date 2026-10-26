use super::*;

#[test]
fn ownership_preflight_ignores_unrelated_mcp_data_but_refuses_invalid_skills() {
    for (skills, accepted) in [
        (json!([]), true),
        (json!("invalid skill declarations"), false),
        (json!([{"name": "custom", "path": 7}]), false),
        (
            json!([{"name": "agenda", "path": "/unread-machine-source"}]),
            false,
        ),
    ] {
        let fixture = Fixture::new(env!("CARGO_PKG_VERSION"));
        let path = fixture.home.join(".config/brain/env.json");
        let mut registry = read_json(&path);
        registry["workspaces"]["alpha"]["env"]["agent_capabilities"] =
            json!({"mcps": "invalid unrelated MCP material", "skills": skills});
        write_json(&path, &registry);
        let output = fixture.run(&["server", "status"]);
        assert_eq!(
            output.status.success(),
            accepted,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fixture.root.join("AGENTS.md").exists(), accepted);
    }
}

#[test]
fn a_verified_current_package_does_not_create_sync_state_for_ordinary_startup() {
    let fixture = Fixture::new(env!("CARGO_PKG_VERSION"));
    fixture.startup();
    let sync = fixture.cache().join("sync");
    if sync.exists() {
        std::fs::remove_dir(&sync).unwrap();
    }
    fixture.startup();
    assert!(
        !sync.exists(),
        "current package verification must not initialize sync state"
    );
    std::fs::create_dir(&sync).unwrap();
    let stale = format!("{} stale-owner", std::process::id());
    std::fs::write(sync.join("sync.lock"), &stale).unwrap();
    fixture.startup();
    assert_eq!(
        std::fs::read_to_string(sync.join("sync.lock")).unwrap(),
        stale
    );
}

#[test]
fn current_portable_generation_never_regrants_a_narrowed_selection() {
    let fixture = Fixture::new("0.105.3");
    write_json(
        &fixture.root.join(".config/config.json"),
        &json!({"allowed_skills": ["todo"]}),
    );
    fixture.startup();
    write_json(
        &fixture.root.join(".config/config.json"),
        &json!({"allowed_skills": ["todo"]}),
    );
    let manifest_path = fixture.cache().join("skills-install/managed.json");
    let mut stale = read_json(&manifest_path);
    stale["binary_version"] = json!("0.105.3");
    write_json(&manifest_path, &stale);
    std::fs::write(
        fixture.home.join(".config/brain/migrations/version"),
        "0.105.3",
    )
    .unwrap();
    fixture.startup();
    assert_eq!(
        read_json(&fixture.root.join(".config/config.json"))["allowed_skills"],
        json!(["todo"])
    );
    std::fs::remove_file(manifest_path).unwrap();
    fixture.startup();
    assert_eq!(
        read_json(&fixture.root.join(".config/config.json"))["allowed_skills"],
        json!(["todo"])
    );
}

#[test]
fn absent_receipts_without_old_generation_evidence_preserve_explicit_scope() {
    let fixture = Fixture::new(env!("CARGO_PKG_VERSION"));
    write_json(
        &fixture.root.join(".config/config.json"),
        &json!({"allowed_skills": ["todo"]}),
    );
    fixture.startup();
    assert_eq!(
        read_json(&fixture.root.join(".config/config.json"))["allowed_skills"],
        json!(["todo"])
    );
}

#[test]
fn source_first_refresh_preserves_authenticated_current_scope_despite_old_metadata_version() {
    let fixture = Fixture::new("0.105.3");
    fixture.startup();
    write_json(
        &fixture.root.join(".config/config.json"),
        &json!({"allowed_skills": ["todo"]}),
    );
    let path = fixture.cache().join("skills-install/managed.json");
    let mut manifest = read_json(&path);
    manifest["binary_version"] = json!("0.105.3");
    write_json(&path, &manifest);
    std::fs::create_dir_all(fixture.root.join(".config/extensions")).unwrap();
    std::fs::write(
        fixture.root.join(".config/extensions/workspace.md"),
        "[workspace:context]\nSource arrived first.\n",
    )
    .unwrap();
    fixture.startup();
    assert_eq!(
        read_json(&fixture.root.join(".config/config.json"))["allowed_skills"],
        json!(["todo"])
    );
}

#[test]
fn newly_registered_root_does_not_inherit_an_old_machine_generation() {
    for action in ["create", "attach"] {
        let fixture = Fixture::new("0.102.24");
        let requested = fixture.temp.path().join("new-root");
        if action == "attach" {
            std::fs::create_dir_all(requested.join(".config")).unwrap();
            write_json(
                &requested.join(".config/config.json"),
                &json!({"allowed_skills": ["todo"]}),
            );
            brain::workspace::WorkspaceManifest::new(brain::workspace::WorkspaceId::new())
                .write_new(&requested)
                .unwrap();
        }
        let output = if action == "create" {
            fixture.run(&[
                "workspace",
                "create",
                "--name",
                "new-root",
                "--root",
                requested.to_str().unwrap(),
            ])
        } else {
            fixture.run(&["workspace", "attach", requested.to_str().unwrap()])
        };
        assert!(
            output.status.success(),
            "{action}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        if action == "create" {
            write_json(
                &requested.join(".config/config.json"),
                &json!({"allowed_skills": ["todo"]}),
            );
            std::fs::write(
                fixture.home.join(".config/brain/migrations/version"),
                "0.102.24",
            )
            .unwrap();
            fixture.startup();
        }
        assert_eq!(
            read_json(&requested.join(".config/config.json"))["allowed_skills"],
            json!(["todo"])
        );
    }
}

#[test]
fn exact_legacy_contract_is_positive_evidence_without_a_local_receipt() {
    let fixture = Fixture::new(env!("CARGO_PKG_VERSION"));
    write_json(
        &fixture.root.join(".config/config.json"),
        &json!({"allowed_skills": ["todo"]}),
    );
    std::fs::write(
        fixture.root.join("AGENTS.md"),
        include_bytes!("../../templates/workspace/legacy/AGENTS-v0.105.3.md"),
    )
    .unwrap();
    fixture.startup();
    assert_eq!(
        read_json(&fixture.root.join(".config/config.json"))["allowed_skills"],
        json!(["todo", "agenda"])
    );
}
