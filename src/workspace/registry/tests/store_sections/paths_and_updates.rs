
#[cfg(unix)]
#[test]
fn atomic_save_creates_an_owner_only_destination() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("env.json");

    RegistryStore::save_atomic_to(&path, &registry_with_brain_and_family()).unwrap();

    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn bare_relative_save_uses_the_current_directory() {
    const CHILD_ENV: &str = "BRAIN_REGISTRY_RELATIVE_PATH_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let registry = registry_with_brain_and_family();
        RegistryStore::save_atomic_to(Path::new("env.json"), &registry)
            .expect("relative atomic save");
        assert_eq!(
            RegistryStore::load_from(Path::new("env.json")).unwrap(),
            registry
        );
        return;
    }

    let directory = tempfile::tempdir().expect("tempdir");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "workspace::registry::tests::store::bare_relative_save_uses_the_current_directory",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .current_dir(directory.path())
        .output()
        .expect("child test process");

    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(directory.path().join("env.json").is_file());
}

#[test]
fn bare_relative_path_parent_is_the_current_directory() {
    assert_eq!(
        crate::workspace::registry::store::parent_or_current_dir(Path::new("env.json")),
        Path::new(".")
    );
}

#[test]
fn store_update_returns_the_complete_removed_record() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("env.json");
    let mut registry = registry_with_brain_and_family();
    let original = registry.workspaces.get(&name("family")).unwrap().clone();
    RegistryStore::save_atomic_to(&path, &registry).unwrap();
    let store = RegistryStore::from_path(path.clone());

    let removed = store
        .update(&mut registry, |candidate| candidate.remove("family"))
        .unwrap();

    assert_eq!(removed, original);
    assert_eq!(removed.root, PathBuf::from("/workspaces/family"));
    assert!(!registry.workspaces.contains_key(&name("family")));
    assert_eq!(RegistryStore::load_from(&path).unwrap(), registry);
}
