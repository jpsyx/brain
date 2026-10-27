use super::*;

#[test]
fn old_flat_registry_is_prepared_before_package_eligibility_is_consumed() {
    let fixture = Fixture::new("0.102.24");
    write_json(
        &fixture.home.join(".config/brain/env.json"),
        &json!({"root": fixture.root}),
    );
    write_json(
        &fixture.root.join(".config/config.json"),
        &json!({"allowed_skills": ["todo"]}),
    );
    let output = fixture.run(&[
        "workspace",
        "repair",
        "--manifest",
        "--local-user-id",
        "member",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        read_json(&fixture.root.join(".config/config.json"))["allowed_skills"],
        json!(["todo", "agenda"])
    );
}

#[test]
fn both_upgrade_paths_prepare_the_complete_package_once() {
    for version in ["0.102.24", "0.105.3"] {
        let fixture = Fixture::new(version);
        write_json(
            &fixture.root.join(".config/config.json"),
            &json!({
                "allowed_skills": ["todo", "second-brain"], "skills_auto_sync": false,
                "unrelated": {"preserve": true}
            }),
        );
        let extensions = fixture.root.join(".config/extensions");
        std::fs::create_dir_all(&extensions).unwrap();
        std::fs::write(
            extensions.join("second-brain.md"),
            "[second-brain:company-context]\nLegacy shared rule.\n",
        )
        .unwrap();
        fixture.startup();
        let document = std::fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap();
        assert!(document.contains("Legacy shared rule."), "{document}");
        let config = read_json(&fixture.root.join(".config/config.json"));
        assert_eq!(
            config["allowed_skills"],
            json!([
                "todo",
                "second-brain",
                "agenda",
                "projects",
                "process-capture",
                "organize",
                "brain-sync"
            ])
        );
        assert_eq!(config["unrelated"], json!({"preserve": true}));
        let manifest = read_json(&fixture.cache().join("skills-install/managed.json"));
        assert_eq!(
            manifest["package"]["config_history"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            manifest["package"]["instructions"]["receipt"]["generated_revision"],
            3
        );
        let before = std::fs::metadata(fixture.root.join("AGENTS.md"))
            .unwrap()
            .modified()
            .unwrap();
        fixture.startup();
        assert_eq!(
            std::fs::metadata(fixture.root.join("AGENTS.md"))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );
        assert_eq!(
            read_json(&fixture.cache().join("skills-install/managed.json")),
            manifest
        );
    }
}

#[test]
fn explicit_installer_from_version_is_positive_legacy_evidence() {
    let fixture = Fixture::new(env!("CARGO_PKG_VERSION"));
    write_json(
        &fixture.root.join(".config/config.json"),
        &json!({"allowed_skills": ["todo"]}),
    );
    let output = fixture.run(&[
        "__migrate",
        "--from-version",
        "0.102.24",
        "--to-version",
        env!("CARGO_PKG_VERSION"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        read_json(&fixture.root.join(".config/config.json"))["allowed_skills"],
        json!(["todo", "agenda"])
    );
    assert_eq!(read_json(&fixture.cache().join("skills-install/managed.json"))["package"]["config_history"].as_array().unwrap().len(), 1);
}

#[test]
fn unavailable_default_and_failed_peer_do_not_hide_coherent_repairs_or_remounts() {
    let fixture = Fixture::new("0.105.3");
    let parked = fixture.temp.path().join("parked");
    std::fs::rename(&fixture.root, &parked).unwrap();
    let registry_path = fixture.home.join(".config/brain/env.json");
    let mut registry = read_json(&registry_path);
    let mut peers = Vec::new();
    for name in ["beta", "gamma"] {
        let root = fixture.temp.path().join(name);
        let id = brain::workspace::WorkspaceId::new();
        brain::workspace::WorkspaceManifest::new(id)
            .write_new(&root)
            .unwrap();
        registry["workspaces"][name] =
            json!({"workspace_id": id, "root": root, "local_user_id":"member", "env":{}});
        peers.push((root, id));
    }
    write_json(&registry_path, &registry);
    let identity = peers[0].0.join(".config/workspace.json");
    let original = std::fs::read(&identity).unwrap();
    std::fs::write(&identity, "invalid identity").unwrap();
    let failed = fixture.run(&["skills", "sync", "--all-workspaces", "--json"]);
    assert!(!failed.status.success());
    let report: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(report["workspaces"][0]["status"], "pending_unavailable");
    assert_eq!(report["workspaces"][1]["status"], "failed");
    assert_eq!(report["workspaces"][2]["status"], "updated");
    assert!(peers[1].0.join("AGENTS.md").is_file());
    assert!(!fixture.root.exists());
    assert!(!fixture.cache().exists());
    std::fs::rename(parked, &fixture.root).unwrap();
    std::fs::write(identity, original).unwrap();
    std::fs::remove_file(peers[1].0.join("AGENTS.md")).unwrap();
    std::fs::write(
        fixture.home.join(".config/brain/migrations/version"),
        "0.106.0",
    )
    .unwrap();
    fixture.startup();
    for root in [&fixture.root, &peers[0].0, &peers[1].0] {
        assert!(root.join("AGENTS.md").is_file());
    }
}

#[test]
fn explicit_repair_reports_earlier_startup_failure_and_still_attempts_package() {
    let fixture = Fixture::new("0.70.0");
    std::fs::write(
        fixture.root.join(".brain"),
        "obstruct lifecycle prerequisite",
    )
    .unwrap();
    let output = fixture.run(&["skills", "sync", "--all-workspaces", "--json"]);
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["startup_error"].is_string());
    assert_eq!(report["workspaces"][0]["status"], "updated");
    assert!(fixture.root.join("AGENTS.md").is_file());
    assert_eq!(
        std::fs::read_to_string(fixture.home.join(".config/brain/migrations/version")).unwrap(),
        "0.70.0"
    );
}

#[test]
fn help_and_version_do_not_publish_or_stamp_workspace_state() {
    for args in [&["--help"][..], &["--version"][..]] {
        let fixture = Fixture::new("0.102.24");
        let registry = std::fs::read(fixture.home.join(".config/brain/env.json")).unwrap();
        assert!(fixture.run(args).status.success());
        assert!(!fixture.root.join("AGENTS.md").exists());
        assert!(!fixture.cache().exists());
        assert_eq!(
            std::fs::read(fixture.home.join(".config/brain/env.json")).unwrap(),
            registry
        );
        assert_eq!(
            std::fs::read_to_string(fixture.home.join(".config/brain/migrations/version")).unwrap(),
            "0.102.24"
        );
    }
}

#[test]
fn old_pointer_only_root_preserves_positive_legacy_identity_through_setup() {
    let fixture = Fixture::new("0.102.24");
    std::fs::remove_file(fixture.home.join(".config/brain/env.json")).unwrap();
    std::fs::write(
        fixture.home.join(".config/brain-root"),
        fixture.root.to_str().unwrap(),
    )
    .unwrap();
    write_json(
        &fixture.root.join(".config/config.json"),
        &json!({"allowed_skills": ["todo"]}),
    );
    let output = fixture.run(&[
        "workspace",
        "repair",
        "--manifest",
        "--local-user-id",
        "member",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        read_json(&fixture.root.join(".config/config.json"))["allowed_skills"],
        json!(["todo", "agenda"])
    );
}

#[test]
fn explicit_patch_upgrade_repairs_the_package_without_crossing_a_feature_callback() {
    let fixture = Fixture::new("0.106.0");
    let output = fixture.run(&[
        "__migrate",
        "--from-version",
        "0.106.0",
        "--to-version",
        env!("CARGO_PKG_VERSION"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fixture.root.join("AGENTS.md").is_file(),
        "target stamp requires a complete package"
    );
}
