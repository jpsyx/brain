use super::{Fixture, assert_ok, legacy_root, refresh, support};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
#[test]
#[ignore = "requires an explicitly supplied audited legacy executable; offline defaults use portable fixtures"]
fn actual_legacy_target_uses_private_sources_and_preserves_live_state() {
    let executable = std::path::PathBuf::from(
        std::env::var_os("BRAIN_TEST_LEGACY_BINARY").expect("explicit legacy fixture binary"),
    );
    let mut f = Fixture::new();
    let root = legacy_root(&mut f, "alpha");
    let missing = legacy_root(&mut f, "missing");
    fs::remove_dir_all(&missing).unwrap();
    let plugin = root.join(".config/plugins/local-example");
    fs::create_dir_all(plugin.join("references/nested")).unwrap();
    fs::write(plugin.join("SKILL.md"), "# Private fixture plugin\n").unwrap();
    fs::write(plugin.join("references/nested/data.bin"), [0, 255, 2, 128]).unwrap();
    f.registry["env"] =
        serde_json::json!({"server_url":"http://127.0.0.1:1", "claude_cmd":"must-not-run"});
    f.registry["workspaces"]["alpha"]["receiver_enabled"] = true.into();
    f.registry["workspaces"]["alpha"]["env"] =
        serde_json::json!({"sync":{"remote":"sentinel:never"}});
    f.save();
    assert!(!refresh(&f, &executable, "preflight").status.success());
    f.registry["workspaces"]
        .as_object_mut()
        .unwrap()
        .remove("missing");
    f.save();
    let before_registry = fs::read(f.registry_path()).unwrap();
    let before_config = fs::read(root.join(".config/config.json")).unwrap();
    let before_source = fs::read(root.join(".config/extensions/todo.md")).unwrap();
    let preflight = refresh(&f, &executable, "preflight");
    assert_ok(&preflight);
    assert!(!root.join("tasks").exists());
    assert!(!root.join(".brain").exists());
    assert!(!root.join(".agents").exists());
    assert!(!missing.exists());
    assert_ok(&refresh(&f, &executable, "apply"));
    let todo = fs::read_to_string(support::todo(&root)).unwrap();
    assert!(todo.contains("alpha extension"));
    assert!(!todo.contains("brain tasks remove T17 --dry-run --json"));
    assert!(
        include_str!("../../skills/todo/SKILL.md")
            .contains("brain tasks remove T17 --dry-run --json")
    );
    assert_eq!(
        fs::read(root.join(".agents/skills/local-example/references/nested/data.bin")).unwrap(),
        [0, 255, 2, 128]
    );
    assert_eq!(fs::read(f.registry_path()).unwrap(), before_registry);
    assert_eq!(
        fs::read(root.join(".config/config.json")).unwrap(),
        before_config
    );
    assert_eq!(
        fs::read(root.join(".config/extensions/todo.md")).unwrap(),
        before_source
    );
    assert!(!root.join("tasks").exists());
    assert!(!root.join(".brain").exists());
    assert!(!missing.exists());
    let db = rusqlite::Connection::open(f.cache("alpha").join("state.db")).unwrap();
    let receipt: String = db
        .query_row(
            "SELECT value FROM meta WHERE key='skills_install_receipt'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let receipt: serde_json::Value = serde_json::from_str(&receipt).unwrap();
    assert_eq!(receipt["version"]["binary_version"], "0.101.0");
    assert_eq!(receipt["version"]["compatibility_revision"], 0);
}

#[test]
#[ignore = "requires an explicitly supplied audited legacy executable"]
fn actual_legacy_target_downgrade_preserves_modern_owned_extras() {
    let old = std::path::PathBuf::from(
        std::env::var_os("BRAIN_TEST_LEGACY_BINARY").expect("explicit legacy fixture binary"),
    );
    let mut f = Fixture::new();
    let root = legacy_root(&mut f, "alpha");
    let current = Path::new(env!("CARGO_BIN_EXE_brain"));
    let digest = format!("{:x}", Sha256::digest(fs::read(current).unwrap()));
    let output = f
        .command()
        .args(["__refresh-workspace-skills", "--target-binary"])
        .arg(current)
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
            .contains("brain tasks remove T17 --dry-run --json")
    );
    let extra = root.join(".agents/skills/todo/local-extra.bin");
    fs::write(&extra, [7, 255, 128]).unwrap();
    let user_skill = root.join(".agents/skills/user-notes");
    fs::create_dir_all(&user_skill).unwrap();
    fs::write(user_skill.join("SKILL.md"), "# User owned\n").unwrap();
    let manifest = f.cache("alpha").join("skills-install/managed.json");
    let before = fs::read(&manifest).unwrap();
    assert_ok(&refresh(&f, &old, "preflight"));
    assert_eq!(fs::read(&manifest).unwrap(), before);
    assert_ok(&f.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.101.0",
        "--installer-refresh-protocol",
        "1",
    ]));
    assert_eq!(fs::read(&manifest).unwrap(), before);
    assert_ok(&refresh(&f, &old, "apply"));
    assert!(
        !fs::read_to_string(support::todo(&root))
            .unwrap()
            .contains("brain tasks remove T17 --dry-run --json")
    );
    assert_eq!(fs::read(extra).unwrap(), [7, 255, 128]);
    assert_eq!(
        fs::read_to_string(user_skill.join("SKILL.md")).unwrap(),
        "# User owned\n"
    );
    assert!(
        !fs::read_link(root.join(".codex/skills/user-notes"))
            .unwrap()
            .is_absolute()
    );
}
