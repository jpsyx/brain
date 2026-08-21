// Schema v2 -> v3: `markdown_to_pdf_path` becomes machine-global, in place, on
// the next `brain` command a user happens to run.

/// A schema-v2 registry with two workspaces, each carrying `env` of its own.
fn v2_registry(config_dir: &Path, roots: &[(&str, &Path)], markdown: &[(&str, &str)]) {
    let workspaces = roots
        .iter()
        .map(|(name, root)| {
            let mut env = json!({"claude_cmd": format!("claude --{name}")});
            if let Some((_, path)) = markdown.iter().find(|(owner, _)| owner == name) {
                env["markdown_to_pdf_path"] = json!(path);
            }
            (
                (*name).to_owned(),
                json!({
                    "workspace_id": WorkspaceId::new().to_string(),
                    "root": root,
                    "aliases": [],
                    "local_user_id": "migration-user",
                    "receiver_enabled": false,
                    "env": env,
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    fs::write(
        config_dir.join("env.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 2,
            "default_workspace": roots[0].0,
            "workspaces": workspaces,
        }))
        .expect("serialize v2 registry"),
    )
    .expect("write v2 registry");
}

#[test]
fn the_next_command_upgrades_a_v2_registry_and_hoists_the_markdown_path() {
    let (home, config_home, config_dir) = fixture_dirs();
    let brain_root = home.path().join("brain");
    let family_root = home.path().join("family");
    fs::create_dir_all(&brain_root).expect("brain root");
    fs::create_dir_all(&family_root).expect("family root");
    v2_registry(
        &config_dir,
        &[("brain", &brain_root), ("family", &family_root)],
        &[("family", "/opt/family/markdown-to-pdf")],
    );

    // Any ordinary command; the user never asks for a migration.
    run_migration(home.path(), config_home.path());

    let registry =
        RegistryStore::load_from(&config_dir.join("env.json")).expect("upgraded registry loads");
    assert_eq!(registry.schema_version, REGISTRY_SCHEMA_VERSION);
    assert_eq!(
        registry.env["markdown_to_pdf_path"],
        "/opt/family/markdown-to-pdf"
    );
    // Both records keep everything else and lose their private copy.
    assert_eq!(registry.workspaces.len(), 2);
    for (name, record) in &registry.workspaces {
        assert_eq!(record.env["claude_cmd"], format!("claude --{name}"));
        assert!(
            !record.env.contains_key("markdown_to_pdf_path"),
            "{name} kept a workspace-scoped markdown_to_pdf_path"
        );
    }
}

#[test]
fn several_configured_paths_collapse_onto_the_first_and_the_rest_are_dropped() {
    let (home, config_home, config_dir) = fixture_dirs();
    let brain_root = home.path().join("brain");
    let family_root = home.path().join("family");
    fs::create_dir_all(&brain_root).expect("brain root");
    fs::create_dir_all(&family_root).expect("family root");
    v2_registry(
        &config_dir,
        &[("brain", &brain_root), ("family", &family_root)],
        &[
            ("brain", "/opt/brain/markdown-to-pdf"),
            ("family", "/opt/family/markdown-to-pdf"),
        ],
    );

    run_migration(home.path(), config_home.path());

    let registry =
        RegistryStore::load_from(&config_dir.join("env.json")).expect("upgraded registry loads");
    // First in canonical-name order wins, so the result never depends on which
    // machine or which command triggered the upgrade.
    assert_eq!(
        registry.env["markdown_to_pdf_path"],
        "/opt/brain/markdown-to-pdf"
    );
    assert_eq!(registry.env.len(), 1);
}

#[test]
fn the_upgrade_backs_up_the_exact_previous_bytes_and_is_idempotent() {
    let (home, config_home, config_dir) = fixture_dirs();
    let brain_root = home.path().join("brain");
    fs::create_dir_all(&brain_root).expect("brain root");
    v2_registry(
        &config_dir,
        &[("brain", &brain_root)],
        &[("brain", "/opt/markdown-to-pdf")],
    );
    let previous = fs::read(config_dir.join("env.json")).expect("read v2 registry");

    run_migration(home.path(), config_home.path());

    let backups = backup_files(&config_dir);
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert_eq!(
        fs::read(&backups[0]).expect("read backup"),
        previous,
        "the pre-upgrade bytes must be recoverable verbatim"
    );

    // A second run has nothing to upgrade: same bytes, no second backup.
    let upgraded = fs::read(config_dir.join("env.json")).expect("read upgraded registry");
    run_migration(home.path(), config_home.path());
    assert_eq!(
        fs::read(config_dir.join("env.json")).expect("read registry again"),
        upgraded
    );
    assert_eq!(backup_files(&config_dir).len(), 1);
}

#[test]
fn a_v2_machine_that_never_set_a_markdown_path_upgrades_without_inventing_one() {
    let (home, config_home, config_dir) = fixture_dirs();
    let brain_root = home.path().join("brain");
    fs::create_dir_all(&brain_root).expect("brain root");
    v2_registry(&config_dir, &[("brain", &brain_root)], &[]);

    run_migration(home.path(), config_home.path());

    let registry =
        RegistryStore::load_from(&config_dir.join("env.json")).expect("upgraded registry loads");
    assert_eq!(registry.schema_version, REGISTRY_SCHEMA_VERSION);
    assert!(registry.env.is_empty());
    assert!(
        !fs::read_to_string(config_dir.join("env.json"))
            .expect("read registry")
            .contains("\"env\": {}"),
        "an empty machine-global map is omitted from the file"
    );
}

#[test]
fn every_workspace_reads_the_one_machine_global_path_after_the_upgrade() {
    let (home, config_home, config_dir) = fixture_dirs();
    let brain_root = home.path().join("brain");
    let family_root = home.path().join("family");
    fs::create_dir_all(&brain_root).expect("brain root");
    fs::create_dir_all(&family_root).expect("family root");
    v2_registry(
        &config_dir,
        &[("brain", &brain_root), ("family", &family_root)],
        &[("family", "/opt/family/markdown-to-pdf")],
    );
    run_migration(home.path(), config_home.path());
    // Readiness is per workspace; the upgrade itself is not.
    let repaired = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args([
            "workspace",
            "repair",
            "-w",
            "family",
            "--manifest",
            "--local-user-id",
            "migration-user",
        ])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("repair family");
    assert!(
        repaired.status.success(),
        "{}",
        String::from_utf8_lossy(&repaired.stderr)
    );

    // The value was only ever configured under `family`; after the upgrade both
    // workspaces resolve it, because it describes the machine.
    for workspace in ["brain", "family"] {
        let output = Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(["env", "get", "markdown_to_pdf_path", "-w", workspace])
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", config_home.path())
            .env("NO_COLOR", "1")
            .output()
            .expect("read env");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "/opt/family/markdown-to-pdf",
            "{workspace}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Rewrite a current registry back to schema v2, pushing machine-global values
/// into the default record — an installation that predates the upgrade, but
/// whose workspace is otherwise fully set up.
fn downgrade_to_v2(config_dir: &Path) {
    let path = config_dir.join("env.json");
    let mut registry: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("read registry")).expect("registry JSON");
    let global = registry
        .as_object_mut()
        .expect("registry object")
        .remove("env")
        .unwrap_or_else(|| json!({}));
    let default = registry["default_workspace"]
        .as_str()
        .expect("default workspace")
        .to_owned();
    for (key, value) in global.as_object().expect("global env object") {
        registry["workspaces"][&default]["env"][key] = value.clone();
    }
    registry["schema_version"] = json!(2);
    fs::write(&path, serde_json::to_vec_pretty(&registry).expect("serialize")).expect("write v2");
}

#[test]
fn a_read_only_status_reads_an_old_schema_without_upgrading_it() {
    let (home, config_home, config_dir) = fixture_dirs();
    let brain_root = home.path().join("brain");
    fs::create_dir_all(&brain_root).expect("brain root");
    v2_registry(
        &config_dir,
        &[("brain", &brain_root)],
        &[("brain", "/opt/markdown-to-pdf")],
    );
    // Make the workspace ready (which upgrades the schema), then put the
    // registry back on the old schema so only the schema is stale.
    run_migration(home.path(), config_home.path());
    downgrade_to_v2(&config_dir);
    let before = fs::read(config_dir.join("env.json")).expect("read v2 registry");

    // `workspace list` is a literal read-only probe: it must neither fail on an
    // old schema nor perform the write that an ordinary command performs.
    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["workspace", "list"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1")
        .output()
        .expect("run workspace list");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("brain"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        fs::read(config_dir.join("env.json")).expect("read registry after"),
        before,
        "a read-only probe must not rewrite the registry"
    );
    // The earlier ready-making run left one backup; the read-only probe adds none.
    assert_eq!(backup_files(&config_dir).len(), 1);
}
