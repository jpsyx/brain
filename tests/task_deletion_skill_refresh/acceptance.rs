use super::{Fixture, assert_ok, legacy_root, refresh, support, target};
use std::fs;

#[test]
fn acceptance_two_roots_upgrade_downgrade_and_remount_preserve_customizations() {
    let mut f = Fixture::new();
    let alpha = legacy_root(&mut f, "alpha");
    let beta = legacy_root(&mut f, "beta");
    let originals: Vec<_> = [&alpha, &beta]
        .map(|root| fs::read(root.join(".config/extensions/todo.md")).unwrap())
        .into();
    let parked = f.root("parked-beta");
    fs::rename(&beta, &parked).unwrap();
    assert_ok(&f.sync());
    assert!(!beta.exists());
    assert!(!f.cache("beta").exists());
    assert!(
        fs::read_to_string(support::todo(&alpha))
            .unwrap()
            .contains("brain tasks remove T17 --dry-run --json")
    );
    fs::rename(&parked, &beta).unwrap();
    assert_ok(&f.sync());
    for ((root, name), original) in [(&alpha, "alpha"), (&beta, "beta")]
        .into_iter()
        .zip(&originals)
    {
        let text = fs::read_to_string(support::todo(root)).unwrap();
        assert_eq!(
            fs::read(root.join(".config/extensions/todo.md")).unwrap(),
            *original,
            "{name}"
        );
        assert!(text.contains("brain tasks remove T17 --dry-run --json"));
    }
    fs::rename(&beta, &parked).unwrap();
    let beta_cache = f.cache("beta");
    let before_receipt = fs::read(beta_cache.join("skills-install/managed.json")).unwrap();
    let executable = f.root("old target");
    target(&executable);
    assert!(!refresh(&f, &executable, "preflight").status.success());
    assert!(!beta.exists());
    assert_eq!(
        fs::read(beta_cache.join("skills-install/managed.json")).unwrap(),
        before_receipt
    );
    fs::rename(&parked, &beta).unwrap();
    assert_ok(&refresh(&f, &executable, "preflight"));
    assert_ok(&f.run(&[
        "__migrate",
        "--from-version",
        env!("CARGO_PKG_VERSION"),
        "--to-version",
        "0.101.0",
        "--installer-refresh-protocol",
        "1",
    ]));
    assert_ok(&refresh(&f, &executable, "apply"));
    for (index, (root, name)) in [(&alpha, "alpha"), (&beta, "beta")].into_iter().enumerate() {
        assert_eq!(
            fs::read_to_string(support::todo(root)).unwrap(),
            format!("legacy target body\n{name} extension")
        );
        assert_eq!(
            fs::read(root.join(".config/extensions/todo.md")).unwrap(),
            originals[index]
        );
        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(".config/config.json")).unwrap()).unwrap();
        assert_eq!(config["skills_auto_sync"], false);
        let db = rusqlite::Connection::open(f.cache(name).join("state.db")).unwrap();
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
}
