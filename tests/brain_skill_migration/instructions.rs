use super::*;

#[test]
fn explicit_sandbox_publishes_instructions_and_preserves_unchanged_generation() {
    let temp = tempfile::tempdir().unwrap();
    let layout = brain::skills::layout::Layout::under_root(temp.path());
    let sources = brain::skills::install::Sources::default();
    brain::skills::install::sync(&layout, &sources).unwrap();
    let path = temp.path().join("AGENTS.md");
    assert!(path.is_file(), "sandbox must publish the full package");
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    brain::skills::install::sync(&layout, &sources).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn missing_instructions_repair_at_equal_version_without_enabling_optional_sync() {
    let fixture = Fixture::new("0.105.3");
    write_json(
        &fixture.root.join(".config/config.json"),
        &json!({"skills_auto_sync": false}),
    );
    fixture.startup();
    std::fs::remove_file(fixture.root.join("AGENTS.md")).unwrap();
    fixture.startup();
    assert!(fixture.root.join("AGENTS.md").is_file());
    assert_eq!(
        read_json(&fixture.root.join(".config/config.json"))["skills_auto_sync"],
        false
    );
}

#[test]
fn explicit_all_root_refresh_is_current_without_rewriting_unchanged_instructions() {
    let fixture = Fixture::new("0.105.3");
    fixture.startup();
    let before = std::fs::metadata(fixture.root.join("AGENTS.md"))
        .unwrap()
        .modified()
        .unwrap();
    let output = fixture.run(&["skills", "sync", "--all-workspaces", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["workspaces"][0]["status"], "current");
    assert_eq!(
        std::fs::metadata(fixture.root.join("AGENTS.md"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
}

#[test]
fn portable_publication_waits_outside_install_ownership_while_sync_is_live() {
    let fixture = Fixture::new("0.105.3");
    let lock = fixture.cache().join("sync/sync.lock");
    let _guard = brain::sync::lock::try_acquire(&lock).unwrap();
    let output = fixture.run(&["skills", "sync", "--all-workspaces", "--json"]);
    assert!(
        !output.status.success(),
        "managed installation must not race portable receive"
    );
    assert!(!fixture.root.join("AGENTS.md").exists());
    assert!(!fixture.cache().join("skills-install/install.lock").exists());
}

#[test]
fn generated_only_initial_root_pulls_before_para_seeding() {
    let transfer = super::sync_fixture::Transfer::new();
    std::fs::remove_dir_all(transfer.fixture.root.join("tasks")).unwrap();
    let output = transfer.run(&["config", "list", "-w", "alpha"], "initial");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(transfer.transfers(), 1);
    assert!(transfer.fixture.root.join("areas").is_dir());
    assert!(
        transfer
            .fixture
            .root
            .join("projects/projects-lookup.csv")
            .is_file()
    );
    assert_eq!(transfer.phase(), None);
}

#[test]
fn unpublished_optional_context_does_not_make_a_generated_only_root_nonempty() {
    let fixture = Fixture::new("0.105.3");
    write_json(
        &fixture.root.join(".config/config.json"),
        &json!({"skills_auto_sync":false}),
    );
    fixture.startup();
    std::fs::create_dir_all(fixture.root.join(".config/extensions")).unwrap();
    std::fs::write(
        fixture.root.join(".config/extensions/workspace.md"),
        "[workspace:context]\nUnpublished optional context.\n",
    )
    .unwrap();
    let output = fixture.run(&["config", "list", "-w", "alpha"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fixture.root.join("areas").is_dir(),
        "a verified generated-only document cannot suppress seeding"
    );
    assert!(
        !std::fs::read_to_string(fixture.root.join("AGENTS.md"))
            .unwrap()
            .contains("Unpublished optional context.")
    );
}

#[test]
fn retained_extension_snapshot_state_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::new("0.105.3");
    let output = Command::new("sh")
        .args([
            "-c",
            "umask 000; exec \"$@\"",
            "fixture",
            env!("CARGO_BIN_EXE_brain"),
            "server",
            "status",
        ])
        .env("HOME", &fixture.home)
        .env("XDG_CONFIG_HOME", fixture.home.join(".config"))
        .env_remove("BRAIN_WORKSPACE")
        .env_remove("BRAIN_WORKSPACE_ID")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = fixture.cache().join("skills-install/managed.json");
    assert!(read_json(&manifest)["package"]["extension_snapshot"].is_object());
    assert_eq!(
        std::fs::metadata(manifest).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn modified_or_unknown_generated_documents_are_repaired_before_para_creation() {
    for unknown in [false, true] {
        let fixture = Fixture::new("0.105.3");
        fixture.startup();
        let path = fixture.root.join("AGENTS.md");
        let original = std::fs::read_to_string(&path).unwrap();
        let changed = if unknown {
            original.replace("schema=1", "schema=99")
        } else {
            original.replace("# Brain workspace", "# Edited workspace")
        };
        assert_ne!(changed, original);
        std::fs::write(&path, &changed).unwrap();
        let output = fixture.run(&["config", "list", "-w", "alpha"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(fixture.root.join("areas").exists());
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }
}

#[test]
fn startup_replaces_oversized_inline_rules_with_core_and_extension_sources() {
    let fixture = Fixture::new("0.106.17");
    let inline = "Obsolete inline rule.\n".repeat(2_000);
    std::fs::write(fixture.root.join("AGENTS.md"), &inline).unwrap();
    let source = fixture.root.join(".config/extensions/workspace.md");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "[workspace:context]\nCustom source rule.\n").unwrap();
    fixture.startup();
    let document = std::fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap();
    assert!(!document.contains("Obsolete inline rule."));
    assert!(document.contains("Custom source rule."));
    assert!(document.len() < 20 * 1024);
    assert_eq!(
        std::fs::read_to_string(source).unwrap(),
        "[workspace:context]\nCustom source rule.\n"
    );
}

#[test]
fn startup_removes_old_prefixes_after_an_earlier_managed_migration() {
    let fixture = Fixture::new("0.106.17");
    fixture.startup();
    let path = fixture.root.join("AGENTS.md");
    let generated = std::fs::read(&path).unwrap();
    let mixed = [b"Old prefix\n".as_slice(), &generated, b"Old suffix\n"].concat();
    std::fs::write(&path, mixed).unwrap();
    let manifest_path = fixture.cache().join("skills-install/managed.json");
    let mut manifest = read_json(&manifest_path);
    manifest["package"]["instructions"]["receipt"]["generated_revision"] = json!(2);
    write_json(&manifest_path, &manifest);
    fixture.startup();
    assert_eq!(std::fs::read(&path).unwrap(), generated);
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    fixture.startup();
    assert_eq!(
        std::fs::metadata(path).unwrap().modified().unwrap(),
        modified
    );
}

#[test]
fn startup_removes_appended_rules_at_equal_version() {
    let fixture = Fixture::new(env!("CARGO_PKG_VERSION"));
    fixture.startup();
    let path = fixture.root.join("AGENTS.md");
    let generated = std::fs::read(&path).unwrap();
    let mixed = [
        generated.as_slice(),
        b"Inline customization is not a source.\n",
    ]
    .concat();
    std::fs::write(&path, mixed).unwrap();
    fixture.startup();
    assert_eq!(std::fs::read(path).unwrap(), generated);
}
