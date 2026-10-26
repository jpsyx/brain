#[path = "task_deletion_skill_migration/support.rs"]
mod support;
use sha2::Digest as _;
use std::fs;
use support::{Fixture, assert_ok, document, todo};

#[test]
fn all_roots_refresh_with_disabled_auto_sync_and_preserve_sources_and_globals() {
    let mut f = Fixture::new();
    let beta = f.add("beta");
    let alpha = f.add("alpha");
    for root in [&alpha, &beta] {
        let source = root.join(".config/extensions/todo.md");
        let mut bytes = fs::read(&source).unwrap();
        bytes.extend_from_slice(
            format!(
                "\n[todo:linear-backlog]\n{} task customization\n",
                root.file_name().unwrap().to_str().unwrap()
            )
            .as_bytes(),
        );
        fs::write(&source, bytes).unwrap();
        fs::create_dir_all(root.join(".agents/skills/todo")).unwrap();
        fs::write(todo(root), "legacy managed todo").unwrap();
        fs::write(
            root.join(".agents/skills/todo/.brain-rendered"),
            "0.102.0\n",
        )
        .unwrap();
    }

    let global = f.temp.path().join("home/.agents/skills/todo");
    fs::create_dir_all(&global).unwrap();
    fs::write(global.join("SKILL.md"), "global sentinel").unwrap();
    let other_global = f.temp.path().join("home/.codex/skills/todo");
    fs::create_dir_all(&other_global).unwrap();
    fs::write(other_global.join("SKILL.md"), "other sentinel").unwrap();
    let originals: Vec<_> = [&alpha, &beta]
        .into_iter()
        .map(|root| fs::read(root.join(".config/extensions/todo.md")).unwrap())
        .collect();
    let output = f.sync();
    assert_ok(&output);
    let value = document(&output);
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["workspaces"][0]["name"], "alpha");
    assert_eq!(value["workspaces"][1]["name"], "beta");
    for (index, root) in [&alpha, &beta].into_iter().enumerate() {
        assert_eq!(value["workspaces"][index]["status"], "updated");
        let text = fs::read_to_string(todo(root)).unwrap();
        assert!(!text.contains("legacy managed todo"));
        assert!(text.contains(&format!(
            "{} task customization",
            root.file_name().unwrap().to_str().unwrap()
        )));
        assert_eq!(
            fs::read(root.join(".config/extensions/todo.md")).unwrap(),
            originals[index]
        );
        assert!(
            fs::read_to_string(root.join(".config/config.json"))
                .unwrap()
                .contains("false")
        );
    }
    assert_eq!(
        fs::read_to_string(global.join("SKILL.md")).unwrap(),
        "global sentinel"
    );
    assert_eq!(
        fs::read_to_string(other_global.join("SKILL.md")).unwrap(),
        "other sentinel"
    );
}

#[test]
fn receipt_tracks_currentness_and_missing_files_override_it() {
    let mut f = Fixture::new();
    let root = f.add("alpha");
    assert_ok(&f.sync());
    let current = f.sync();
    assert_ok(&current);
    assert_eq!(document(&current)["workspaces"][0]["status"], "current");
    let db = rusqlite::Connection::open(f.cache("alpha").join("state.db")).unwrap();
    let receipt: String = db
        .query_row(
            "SELECT value FROM meta WHERE key='skills_install_receipt'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let receipt: serde_json::Value = serde_json::from_str(&receipt).unwrap();
    assert_eq!(receipt["version"]["compatibility_revision"], 2);
    assert_eq!(
        receipt["version"]["binary_version"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(receipt["manifest_sha256"].as_str().unwrap().len(), 64);
    let manifest = fs::read(f.cache("alpha").join("skills-install/managed.json")).unwrap();
    assert_eq!(
        receipt["manifest_sha256"],
        format!("{:x}", sha2::Sha256::digest(&manifest))
    );
    let mut stale = receipt;
    stale["version"]["compatibility_revision"] = serde_json::json!(0);
    db.execute(
        "UPDATE meta SET value=?1 WHERE key='skills_install_receipt'",
        [serde_json::to_string(&stale).unwrap()],
    )
    .unwrap();
    let out = f.sync();
    assert_ok(&out);
    assert_eq!(document(&out)["workspaces"][0]["status"], "updated");

    fs::remove_file(todo(&root)).unwrap();
    let repaired = f.sync();
    assert_ok(&repaired);
    assert_eq!(document(&repaired)["workspaces"][0]["status"], "updated");
    assert!(todo(&root).is_file());
    fs::write(
        root.join(".config/extensions/todo.md"),
        "[todo:linear-backlog]\nchanged source\n",
    )
    .unwrap();
    let changed = f.sync();
    assert_ok(&changed);
    assert_eq!(document(&changed)["workspaces"][0]["status"], "updated");
    assert!(
        fs::read_to_string(todo(&root))
            .unwrap()
            .contains("changed source")
    );
}

#[test]
fn earlier_startup_failure_keeps_independent_root_outcomes_and_marker() {
    let mut f = Fixture::new();
    let alpha = f.add("alpha");
    let beta = f.add("beta");
    let missing = f.add("gamma");
    fs::rename(&missing, f.root("parked-gamma")).unwrap();
    fs::write(alpha.join(".brain"), "hook obstruction").unwrap();
    fs::create_dir_all(alpha.join(".agents/skills/todo")).unwrap();
    fs::write(todo(&alpha), "protected custom todo").unwrap();
    let stamp = f.temp.path().join("config/brain/migrations/version");
    fs::write(&stamp, "0.70.0").unwrap();
    let out = f.run(&["--verbose", "skills", "sync", "--all-workspaces", "--json"]);
    assert!(!out.status.success());
    let doc = document(&out);
    assert!(!doc["startup_error"].is_null());
    assert_eq!(doc["workspaces"][0]["status"], "failed");
    assert_eq!(doc["workspaces"][1]["status"], "updated");
    assert_eq!(doc["workspaces"][2]["status"], "pending_unavailable");
    assert!(!missing.exists());
    assert!(todo(&beta).is_file());
    assert_eq!(fs::read_to_string(stamp).unwrap(), "0.70.0");
    assert_eq!(
        fs::read_to_string(todo(&alpha)).unwrap(),
        "protected custom todo"
    );
}

#[test]
fn unavailable_default_stays_absent_then_remount_retries() {
    let mut f = Fixture::new();
    let alpha = f.add("alpha");
    let beta = f.add("beta");
    let parked = f.root("parked");
    fs::rename(&alpha, &parked).unwrap();
    let out = f.sync();
    assert_ok(&out);
    let doc = document(&out);
    assert_eq!(doc["workspaces"][0]["status"], "pending_unavailable");
    assert_eq!(doc["workspaces"][1]["status"], "updated");
    assert!(!alpha.exists());
    assert!(!f.cache("alpha").exists());
    assert!(todo(&beta).exists());
    fs::rename(parked, &alpha).unwrap();
    let out = f.sync();
    assert_ok(&out);
    assert_eq!(document(&out)["workspaces"][0]["status"], "updated");
}

#[test]
fn missing_registry_is_empty_success_but_invalid_registry_is_fatal() {
    let f = Fixture::new();
    let out = f.sync();
    assert_ok(&out);
    assert_eq!(document(&out)["workspaces"], serde_json::json!([]));
    assert!(!f.registry_path().exists());
    fs::write(f.registry_path(), b"not-json").unwrap();
    let out = f.sync();
    assert!(!out.status.success());
    assert!(document(&out)["refresh_error"].is_string());
}

#[test]
fn legacy_registry_normalizes_in_memory_and_aliases_do_not_duplicate_roots() {
    let mut f = Fixture::new();
    f.add("alpha");
    f.add("beta");
    f.registry["schema_version"] = serde_json::json!(2);
    f.registry["workspaces"]["alpha"]["aliases"] = serde_json::json!(["first", "primary"]);
    f.save();
    let before = fs::read(f.registry_path()).unwrap();
    let out = f.sync();
    assert_ok(&out);
    assert_eq!(document(&out)["workspaces"].as_array().unwrap().len(), 2);
    assert_eq!(fs::read(f.registry_path()).unwrap(), before);
    f.registry["workspaces"]["beta"]["aliases"] = serde_json::json!(["first"]);
    f.save();
    let out = f.sync();
    assert!(!out.status.success());
    assert!(document(&out)["refresh_error"].is_string());
}

#[test]
fn malformed_and_mismatched_manifests_fail_without_blocking_good_root() {
    for malformed in [true, false] {
        let mut f = Fixture::new();
        let alpha = f.add("alpha");
        let beta = f.add("beta");
        if malformed {
            fs::write(alpha.join(".config/workspace.json"), "bad manifest").unwrap();
        } else {
            fs::copy(
                beta.join(".config/workspace.json"),
                alpha.join(".config/workspace.json"),
            )
            .unwrap();
        }
        let out = f.sync();
        assert!(!out.status.success());
        let doc = document(&out);
        assert_eq!(doc["workspaces"][0]["status"], "failed");
        assert_eq!(doc["workspaces"][1]["status"], "updated");
        assert!(!todo(&alpha).exists());
        assert!(todo(&beta).exists());
    }
}

#[test]
fn failed_receipt_write_retries_committed_install_and_modified_files_fail_closed() {
    let mut f = Fixture::new();
    let root = f.add("alpha");
    fs::create_dir_all(f.cache("alpha").join("state.db")).unwrap();
    let out = f.sync();
    assert!(!out.status.success());
    assert_eq!(document(&out)["workspaces"][0]["status"], "failed");
    assert!(
        f.cache("alpha")
            .join("skills-install/pending.json")
            .is_file()
    );
    let before = fs::metadata(todo(&root)).unwrap().modified().unwrap();
    fs::remove_dir(f.cache("alpha").join("state.db")).unwrap();
    let out = f.sync();
    assert_ok(&out);
    assert_eq!(document(&out)["workspaces"][0]["status"], "updated");
    assert_eq!(
        fs::metadata(todo(&root)).unwrap().modified().unwrap(),
        before
    );
    assert!(
        !f.cache("alpha")
            .join("skills-install/pending.json")
            .exists()
    );
    fs::write(todo(&root), "protected edit").unwrap();
    let out = f.sync();
    assert!(!out.status.success());
    assert_eq!(document(&out)["workspaces"][0]["status"], "failed");
    assert_eq!(fs::read_to_string(todo(&root)).unwrap(), "protected edit");
}

#[test]
fn hooks_only_error_keeps_successful_skill_outcomes_and_ordinary_startup_stops() {
    let mut f = Fixture::new();
    let alpha = f.add("alpha");
    let beta = f.add("beta");
    fs::write(alpha.join(".brain"), "hook obstruction").unwrap();
    let ordinary = f.run(&["config", "list"]);
    assert!(!ordinary.status.success());
    assert!(!todo(&beta).exists());
    let out = f.sync();
    assert!(!out.status.success());
    let doc = document(&out);
    assert!(doc["startup_error"].is_string());
    assert_eq!(doc["workspaces"][0]["status"], "updated");
    assert_eq!(doc["workspaces"][1]["status"], "updated");
    fs::remove_file(alpha.join(".brain")).unwrap();
    let out = f.sync();
    assert_ok(&out);
    assert_eq!(document(&out)["workspaces"][0]["status"], "current");
}

#[test]
fn incompatible_cli_options_fail_before_work() {
    let mut f = Fixture::new();
    let root = f.add("alpha");
    for args in [
        vec!["skills", "sync", "--all-workspaces", "-w", "alpha"],
        vec![
            "skills",
            "sync",
            "--all-workspaces",
            "--root",
            "/irrelevant",
        ],
        vec!["skills", "sync", "--json"],
    ] {
        let out = f.run(&args);
        assert!(!out.status.success());
        assert!(!todo(&root).exists());
    }
}

#[test]
fn diagnostic_messages_do_not_expose_invalid_manifest_content() {
    let mut f = Fixture::new();
    let root = f.add("alpha");
    fs::write(
        root.join(".config/workspace.json"),
        r#"{"schema_version":"private-invalid-value","workspace_id":"private-invalid-uuid"}"#,
    )
    .unwrap();
    let out = f.sync();
    assert!(!out.status.success());
    assert_eq!(document(&out)["workspaces"][0]["status"], "failed");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("private-invalid"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("private-invalid"));
}

#[test]
fn inherited_selector_and_strict_child_env_do_not_select_a_root_for_repair() {
    let mut f = Fixture::new();
    f.add("alpha");
    let out = f
        .command()
        .args(["skills", "sync", "--all-workspaces", "--json"])
        .env("BRAIN_WORKSPACE", "missing-selector")
        .env("BRAIN_REQUIRE_WORKSPACE", "1")
        .env("BRAIN_WORKSPACE_ID", "wrong-inherited-uuid")
        .output()
        .unwrap();
    assert_ok(&out);
    assert_eq!(document(&out)["workspaces"][0]["status"], "updated");
}

#[test]
fn registry_only_bootstrap_never_requires_registry_selection_or_readiness() {
    use brain::workspace::{
        BootstrapContext, BootstrapPolicy, InteractionMode, RegistryStore, bootstrap_policy,
        invocation_for,
    };
    let f = Fixture::new();
    let mut cli =
        brain::cli::try_parse_from(["brain", "skills", "sync", "--all-workspaces"]).unwrap();
    assert_eq!(
        bootstrap_policy(invocation_for(&cli)),
        BootstrapPolicy::RegistryOnly
    );
    let mut output = Vec::new();
    let context = brain::workspace::bootstrap_with_io(
        &mut cli,
        RegistryStore::from_path(f.registry_path()),
        f.temp.path(),
        f.temp.path(),
        InteractionMode::NonInteractive,
        &mut std::io::Cursor::new(Vec::<u8>::new()),
        &mut output,
    )
    .unwrap();
    assert!(matches!(context, BootstrapContext::RegistryOnly(_)));
    assert!(output.is_empty());
    assert!(!f.registry_path().exists());
}

#[test]
fn callback_failure_retains_first_attempt_and_untraversed_callback_falls_back() {
    for traversal in ["reached", "not-traversed"] {
        let mut f = Fixture::new();
        let alpha = f.add("alpha");
        f.add("broken");
        fs::create_dir_all(f.cache("broken").join("state.db")).unwrap();
        let recorded = if traversal == "not-traversed" {
            "0.102.26"
        } else {
            "0.102.24"
        };
        fs::write(f.root("config/brain/migrations/version"), recorded).unwrap();
        let output = f.sync();
        let report = document(&output);
        assert!(!output.status.success());
        assert_eq!(report["workspaces"][0]["status"], "updated", "{report}");
        assert_eq!(report["workspaces"][1]["status"], "failed");
        assert!(todo(&alpha).exists());
        assert!(report["startup_error"].is_string());
        assert_eq!(
            fs::read_to_string(f.root("config/brain/migrations/version")).unwrap(),
            recorded
        );
    }
}

#[path = "task_deletion_skill_migration/diagnostics.rs"]
mod diagnostics;
