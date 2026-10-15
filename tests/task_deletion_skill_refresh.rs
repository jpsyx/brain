#[path = "task_deletion_skill_migration/support.rs"]
mod support;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use support::{Fixture, assert_ok};

#[path = "task_deletion_skill_refresh/fixture.rs"]
mod fixture;
use fixture::{legacy_root, refresh, target};

#[test]
fn historical_migration_rejected_before_machine_state_changes() {
    let f = Fixture::new();
    let stamp = f.root("config/brain/migrations/version");
    let before = fs::read(&stamp).unwrap();
    let output = f.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.101.0",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--target-source"));
    assert_eq!(fs::read(stamp).unwrap(), before);
}
#[test]
fn target_replica_publishes_target_bytes_and_missing_root_stays_pending() {
    let mut f = Fixture::new();
    let root = legacy_root(&mut f, "alpha");
    let missing = legacy_root(&mut f, "missing");
    fs::remove_dir_all(&missing).unwrap();
    let executable = f.root("old target");
    target(&executable);
    let preflight = refresh(&f, &executable, "preflight");
    assert_ok(&preflight);
    assert!(!root.join(".agents").exists());
    assert!(!missing.exists());
    assert_ok(&refresh(&f, &executable, "apply"));
    assert_eq!(
        fs::read_to_string(support::todo(&root)).unwrap(),
        "legacy target body\nalpha extension"
    );
    assert!(!missing.exists());
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(f.cache("alpha").join("skills-install/managed.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["binary_version"], "0.101.0");
    assert_eq!(manifest["compatibility_revision"], 0);
    assert!(
        !fs::read_link(root.join(".codex/skills/todo"))
            .unwrap()
            .is_absolute()
    );
}

#[test]
fn digest_binding_rejects_changed_or_missing_identity_before_target_work() {
    let mut f = Fixture::new();
    let root = legacy_root(&mut f, "alpha");
    let executable = f.root("old target");
    target(&executable);
    let digest = format!("{:x}", Sha256::digest(fs::read(&executable).unwrap()));
    fs::write(
        &executable,
        [
            fs::read(&executable).unwrap(),
            b"\n# changed bytes\n".to_vec(),
        ]
        .concat(),
    )
    .unwrap();
    for digest in [Some(digest.as_str()), Some("bad"), None] {
        let mut cmd = f.command();
        cmd.args(["__refresh-workspace-skills", "--target-binary"])
            .arg(&executable)
            .args(["--target-version", "0.101.0", "--phase", "apply"]);
        if let Some(digest) = digest {
            cmd.args(["--expected-target-sha256", digest]);
        }
        let output = cmd.output().unwrap();
        assert!(!output.status.success());
        assert!(!root.join(".agents").exists());
        assert!(!f.cache("alpha").exists());
    }
}
#[test]
fn protected_owned_edit_remains_protected_after_acknowledged_down() {
    let mut f = Fixture::new();
    let root = legacy_root(&mut f, "alpha");
    assert_ok(&f.sync());
    assert_eq!(
        support::document(&f.sync())["workspaces"][0]["status"],
        "current"
    );
    let ownership = f.cache("alpha").join("skills-install/managed.json");
    let before = fs::read(&ownership).unwrap();
    let down = [
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.101.0",
        "--installer-refresh-protocol",
        "1",
    ];
    assert_ok(&f.run(&down));
    assert_ok(&f.run(&down));
    assert_eq!(fs::read(ownership).unwrap(), before);
    let db = rusqlite::Connection::open(f.cache("alpha").join("state.db")).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM meta WHERE key='skills_install_receipt'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    fs::write(support::todo(&root), "edited owned bytes").unwrap();
    let executable = f.root("old target");
    target(&executable);
    for phase in ["preflight", "apply"] {
        let output = refresh(&f, &executable, phase);
        assert!(!output.status.success());
        let text = String::from_utf8_lossy(&output.stderr);
        for expected in [
            "alpha",
            "managed_content_modified",
            ".agents/skills/todo/SKILL.md",
            "retry",
        ] {
            assert!(text.contains(expected), "{text}");
        }
        assert!(!text.contains("edited owned bytes"));
    }
    assert_eq!(
        fs::read_to_string(support::todo(&root)).unwrap(),
        "edited owned bytes"
    );
}
#[test]
fn source_symlinks_reserved_marker_and_foreign_frontend_fail_before_publication() {
    for fault in ["symlink", "cycle", "marker", "frontend", "core-collision"] {
        let mut f = Fixture::new();
        let root = legacy_root(&mut f, "alpha");
        let plugin = root.join(".config/plugins/local");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join("SKILL.md"), "# local").unwrap();
        match fault {
            "symlink" => {
                fs::write(f.root("secret"), "secret sentinel").unwrap();
                std::os::unix::fs::symlink(f.root("secret"), plugin.join("secret")).unwrap();
            }
            "cycle" => std::os::unix::fs::symlink(&plugin, plugin.join("cycle")).unwrap(),
            "marker" => fs::write(plugin.join(".brain-rendered"), "owned").unwrap(),
            "frontend" => {
                fs::remove_dir_all(&plugin).unwrap();
                fs::create_dir_all(root.join(".codex/skills/todo")).unwrap();
                fs::write(root.join(".codex/skills/todo/SKILL.md"), "foreign").unwrap();
            }
            "core-collision" => fs::rename(&plugin, root.join(".config/plugins/todo")).unwrap(),
            _ => unreachable!(),
        }
        let executable = f.root("old target");
        target(&executable);
        let output = refresh(&f, &executable, "preflight");
        assert!(!output.status.success(), "{fault}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret sentinel"));
        assert!(!root.join(".agents").exists());
    }
}
#[test]
fn current_target_runs_before_bootstrap_and_publishes_revision_one() {
    let mut f = Fixture::new();
    let root = f.add("alpha");
    let executable = Path::new(env!("CARGO_BIN_EXE_brain"));
    let digest = format!("{:x}", Sha256::digest(fs::read(executable).unwrap()));
    let output = f
        .command()
        .args(["__refresh-workspace-skills", "--target-binary"])
        .arg(executable)
        .args([
            "--target-version",
            env!("CARGO_PKG_VERSION"),
            "--phase",
            "apply",
            "--expected-target-sha256",
            &digest,
        ])
        .output()
        .unwrap();
    assert_ok(&output);
    assert!(
        fs::read_to_string(support::todo(&root))
            .unwrap()
            .contains("alpha extension")
    );
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(f.cache("alpha").join("skills-install/managed.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["compatibility_revision"], 1);
    assert!(!root.join("tasks").exists());
}

#[test]
fn target_mutation_unsupported_grammar_and_wrong_version_fail_before_publication() {
    for fault in ["mutated", "grammar", "version"] {
        let mut f = Fixture::new();
        let root = legacy_root(&mut f, "alpha");
        let executable = f.root("old target");
        target(&executable);
        let source = fs::read_to_string(&executable).unwrap();
        let changed = match fault {
            "mutated" => source.replace("registry = json.loads", "with open(__file__, 'a') as changing: changing.write('\\n# changed\\n')\nregistry = json.loads"),
            "grammar" => source.replace("print('Usage: brain skills sync'); sys.exit()", "sys.exit(1)"),
            "version" => source.replace("print('brain 0.101.0')", "print('brain 0.100.0')"),
            _ => unreachable!(),
        };
        fs::write(&executable, changed).unwrap();
        let output = refresh(&f, &executable, "preflight");
        assert!(!output.status.success(), "{fault}");
        assert!(!root.join(".agents").exists());
    }
}
#[test]
fn apply_revalidates_changed_sources_and_attempts_good_roots_after_failure() {
    let mut f = Fixture::new();
    let alpha = legacy_root(&mut f, "alpha");
    let beta = legacy_root(&mut f, "beta");
    let executable = f.root("old target");
    target(&executable);
    assert_ok(&refresh(&f, &executable, "preflight"));
    fs::create_dir_all(alpha.join(".agents/skills/todo")).unwrap();
    fs::write(support::todo(&alpha), "foreign collision").unwrap();
    fs::write(
        beta.join(".config/extensions/todo.md"),
        "[todo:anchors]\nchanged after preflight\n",
    )
    .unwrap();
    let output = refresh(&f, &executable, "apply");
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(support::todo(&alpha)).unwrap(),
        "foreign collision"
    );
    assert_eq!(
        fs::read_to_string(support::todo(&beta)).unwrap(),
        "legacy target body\nchanged after preflight"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("alpha: pending_failed"));
}

#[path = "task_deletion_skill_refresh/actual.rs"]
mod actual;

#[test]
fn incompatible_identity_task_schema_and_minimum_version_reject_without_skills() {
    for fault in ["uuid", "schema", "minimum", "protocol"] {
        let mut f = Fixture::new();
        let root = legacy_root(&mut f, "alpha");
        match fault {
            "uuid" => {
                f.registry["workspaces"]["alpha"]["workspace_id"] =
                    uuid::Uuid::new_v4().to_string().into();
                f.save();
            }
            "schema" => {
                fs::create_dir_all(root.join("tasks")).unwrap();
                fs::write(
                    root.join("tasks/SCHEMA.json"),
                    r#"{"task_schema_version":3}"#,
                )
                .unwrap();
            }
            "minimum" => {
                let path = root.join(".config/workspace.json");
                let mut manifest: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                manifest["minimum_brain_version"] = "99.0.0".into();
                fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            }
            "protocol" => {}
            _ => unreachable!(),
        }
        let executable = f.root("old target");
        target(&executable);
        let output = if fault == "protocol" {
            f.run(&[
                "__migrate",
                "--from-version",
                env!("CARGO_PKG_VERSION"),
                "--to-version",
                "0.101.0",
                "--installer-refresh-protocol",
                "2",
            ])
        } else {
            refresh(&f, &executable, "preflight")
        };
        assert!(!output.status.success(), "{fault}");
        assert!(!root.join(".agents").exists());
    }
}

#[test]
fn redirected_workspace_cache_rejects_before_local_recovery_writes() {
    let mut f = Fixture::new();
    legacy_root(&mut f, "alpha");
    let external = f.root("unrelated-cache");
    fs::create_dir_all(&external).unwrap();
    fs::create_dir_all(f.cache("alpha").parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&external, f.cache("alpha")).unwrap();
    let executable = f.root("old target");
    target(&executable);
    let output = refresh(&f, &executable, "preflight");
    assert!(!output.status.success());
    assert_eq!(fs::read_dir(external).unwrap().count(), 0);
}

#[test]
fn redirected_task_lock_rejects_before_recovery_opens_sqlite() {
    let mut f = Fixture::new();
    legacy_root(&mut f, "alpha");
    let outside = f.root("unrelated-file");
    fs::write(&outside, b"").unwrap();
    fs::create_dir_all(f.cache("alpha")).unwrap();
    std::os::unix::fs::symlink(&outside, f.cache("alpha").join("tasks.transaction.lock")).unwrap();
    let executable = f.root("old target");
    target(&executable);
    let output = refresh(&f, &executable, "preflight");
    assert!(!output.status.success());
    assert!(fs::read(outside).unwrap().is_empty());
}

#[path = "task_deletion_skill_refresh/acceptance.rs"]
mod acceptance;

#[path = "task_deletion_skill_refresh/ordinary_patch.rs"]
mod ordinary_patch;

#[path = "task_deletion_skill_refresh/diagnostics.rs"]
mod diagnostics;
