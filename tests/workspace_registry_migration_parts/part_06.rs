// Schema v3 -> v4: `brain_receiver_public_url` becomes machine-global, in place,
// on the next `brain` command a user happens to run. This is the upgrade a
// machine that already configured a receiver actually takes.

/// A schema-v3 registry: the machine-global map already exists, but every
/// workspace still carries its own copy of the public receiver origin.
fn v3_registry(config_dir: &Path, roots: &[(&str, &Path)], origin: &str) {
    let workspaces = roots
        .iter()
        .map(|(name, root)| {
            (
                (*name).to_owned(),
                json!({
                    "workspace_id": WorkspaceId::new().to_string(),
                    "root": root,
                    "aliases": [],
                    "local_user_id": "migration-user",
                    "receiver_enabled": true,
                    "env": {
                        "brain_receiver_public_url": origin,
                        "twilio_auth_token": format!("token-{name}"),
                        "twilio_from_number": format!("+1310555010{}", name.len()),
                    },
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    fs::write(
        config_dir.join("env.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 3,
            "default_workspace": roots[0].0,
            "env": {"markdown_to_pdf_path": "/opt/markdown-to-pdf"},
            "workspaces": workspaces,
        }))
        .expect("serialize v3 registry"),
    )
    .expect("write v3 registry");
}

#[test]
fn the_next_command_upgrades_a_v3_registry_and_hoists_the_receiver_origin() {
    let (home, config_home, config_dir) = fixture_dirs();
    let brain_root = home.path().join("brain");
    let family_root = home.path().join("family");
    fs::create_dir_all(&brain_root).expect("brain root");
    fs::create_dir_all(&family_root).expect("family root");
    v3_registry(
        &config_dir,
        &[("brain", &brain_root), ("family", &family_root)],
        "https://brain.example.com",
    );

    // Any ordinary command; the user never asks for a migration.
    run_migration(home.path(), config_home.path());

    let registry =
        RegistryStore::load_from(&config_dir.join("env.json")).expect("upgraded registry loads");
    assert_eq!(registry.schema_version, REGISTRY_SCHEMA_VERSION);
    assert_eq!(
        registry.env["brain_receiver_public_url"],
        "https://brain.example.com"
    );
    // The value hoisted earlier is untouched, and provider credentials plus the
    // per-workspace routing number stay exactly where they belong.
    assert_eq!(registry.env["markdown_to_pdf_path"], "/opt/markdown-to-pdf");
    assert_eq!(registry.workspaces.len(), 2);
    for (name, record) in &registry.workspaces {
        assert_eq!(record.env["twilio_auth_token"], format!("token-{name}"));
        assert!(record.env.contains_key("twilio_from_number"));
        assert!(
            !record.env.contains_key("brain_receiver_public_url"),
            "{name} kept a workspace-scoped receiver origin"
        );
    }
    // The exact previous bytes are recoverable, exactly once.
    assert_eq!(backup_files(&config_dir).len(), 1);
}

#[test]
fn after_the_upgrade_every_workspace_prints_the_one_machine_wide_url() {
    let (home, config_home, config_dir) = fixture_dirs();
    let brain_root = home.path().join("brain");
    let family_root = home.path().join("family");
    fs::create_dir_all(&brain_root).expect("brain root");
    fs::create_dir_all(&family_root).expect("family root");
    v3_registry(
        &config_dir,
        &[("brain", &brain_root), ("family", &family_root)],
        "https://brain.example.com/",
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

    for workspace in ["brain", "family"] {
        let output = Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(["receiver", "url", "--sms", "-w", workspace])
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", config_home.path())
            .env("NO_COLOR", "1")
            .output()
            .expect("run receiver url");

        assert!(
            output.status.success(),
            "{workspace}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let printed = String::from_utf8_lossy(&output.stdout);
        // One URL, no ingress, and the stored trailing slash never doubles into
        // a string a provider would sign differently.
        assert!(
            printed.contains("https://brain.example.com/sms"),
            "{workspace}: {printed}"
        );
        assert!(!printed.contains("//sms"), "{workspace}: {printed}");
        assert!(!printed.contains("/w/"), "{workspace}: {printed}");
    }
}
