//! Ordinary patch upgrades must refresh even above the managed migration boundary.
use super::support::{Fixture, assert_ok, todo};
use sha2::{Digest as _, Sha256};
use std::{fs, path::Path};

const PREVIOUS: &str = "0.102.26";

fn stamp(f: &Fixture) -> std::path::PathBuf {
    f.root("config/brain/migrations/version")
}
fn snapshot(f: &Fixture, name: &str, root: &Path) -> Vec<(Vec<u8>, std::time::SystemTime)> {
    [
        todo(root),
        f.cache(name).join("skills-install/managed.json"),
        f.cache(name).join("state.db"),
    ]
    .into_iter()
    .map(|path| {
        (
            fs::read(&path).unwrap(),
            fs::metadata(path).unwrap().modified().unwrap(),
        )
    })
    .collect()
}

#[test]
fn ordinary_patch_upgrade_repairs_all_roots_before_stamp_despite_auto_sync_false() {
    let mut f = Fixture::new();
    let roots = [f.add("alpha"), f.add("beta")];
    assert_ok(&f.sync());
    let expected = roots.map(|root| (root.clone(), fs::read(todo(&root)).unwrap()));
    for (root, _) in &expected {
        fs::remove_file(todo(root)).unwrap();
    }
    fs::write(stamp(&f), PREVIOUS).unwrap();
    assert_ok(&f.run(&["workspace", "list"]));
    for ((root, bytes), name) in expected.iter().zip(["alpha", "beta"]) {
        assert_eq!(fs::read(todo(root)).unwrap(), *bytes, "{name}");
        assert_eq!(
            fs::read_to_string(root.join(".config/config.json")).unwrap(),
            r#"{"skills_auto_sync":false}"#
        );
        let manifest = fs::read(f.cache(name).join("skills-install/managed.json")).unwrap();
        let db = rusqlite::Connection::open(f.cache(name).join("state.db")).unwrap();
        let receipt: String = db
            .query_row(
                "SELECT value FROM meta WHERE key='skills_install_receipt'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let receipt: serde_json::Value = serde_json::from_str(&receipt).unwrap();
        assert_eq!(
            receipt["version"]["binary_version"],
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(receipt["version"]["compatibility_revision"], 1);
        assert_eq!(
            receipt["manifest_sha256"],
            format!("{:x}", Sha256::digest(manifest))
        );
    }
    assert_eq!(
        fs::read_to_string(stamp(&f)).unwrap().trim(),
        env!("CARGO_PKG_VERSION")
    );
    let before = [
        snapshot(&f, "alpha", &expected[0].0),
        snapshot(&f, "beta", &expected[1].0),
    ];
    assert_ok(&f.run(&["workspace", "list"]));
    assert_eq!(
        [
            snapshot(&f, "alpha", &expected[0].0),
            snapshot(&f, "beta", &expected[1].0)
        ],
        before
    );
}

#[test]
fn ordinary_patch_upgrade_failure_blocks_dispatch_and_preserves_old_stamp() {
    for fault in ["owned-edit", "identity"] {
        let mut f = Fixture::new();
        let alpha = f.add("alpha");
        let beta = f.add("beta");
        assert_ok(&f.sync());
        let path = if fault == "owned-edit" {
            todo(&beta)
        } else {
            beta.join(".config/workspace.json")
        };
        fs::write(&path, "foreign content").unwrap();
        fs::write(stamp(&f), PREVIOUS).unwrap();
        let before = snapshot(&f, "alpha", &alpha);
        let output = f.run(&["config", "set", "enable_triage_habits=false", "-w", "alpha"]);
        assert!(!output.status.success(), "{fault}: dispatch must not run");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("compatibility refresh failed"));
        assert_eq!(fs::read_to_string(stamp(&f)).unwrap(), PREVIOUS);
        assert_eq!(fs::read_to_string(path).unwrap(), "foreign content");
        assert_eq!(snapshot(&f, "alpha", &alpha), before);
        assert!(
            !fs::read_to_string(alpha.join(".config/config.json"))
                .unwrap()
                .contains("enable_triage_habits")
        );
    }
}

#[test]
fn ordinary_earlier_traversal_failure_skips_refresh_and_stamp() {
    let mut f = Fixture::new();
    let root = f.add("alpha");
    assert_ok(&f.sync());
    fs::remove_file(todo(&root)).unwrap();
    fs::remove_dir_all(root.join(".brain")).unwrap();
    fs::write(root.join(".brain"), "earlier hook obstruction").unwrap();
    fs::write(stamp(&f), "0.70.0").unwrap();
    let output = f.run(&["workspace", "list"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!todo(&root).exists());
    assert_eq!(fs::read_to_string(stamp(&f)).unwrap(), "0.70.0");
}
