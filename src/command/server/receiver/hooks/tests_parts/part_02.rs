
#[test]
fn merge_is_idempotent_and_preserves_other_settings() {
    let mut settings = json!({"permissions": {"allow": ["Read"]}});
    replace_entry(
        &mut settings,
        "SessionStart",
        "session.py",
        "/tmp/session.py",
    );
    replace_entry(
        &mut settings,
        "SessionStart",
        "session.py",
        "/tmp/session.py",
    );
    assert_eq!(
        settings["hooks"]["SessionStart"].as_array().unwrap().len(),
        1
    );
    assert_eq!(settings["permissions"]["allow"][0], "Read");
}

#[test]
fn concurrent_workspace_registrations_and_unrelated_settings_survive() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hooks.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "permissions": {"allow": ["Read"]}
        }))
        .unwrap(),
    )
    .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

    std::thread::scope(|scope| {
        for workspace in ["family", "work"] {
            let path = path.clone();
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                update_json_file(&path, |settings| {
                    let basename = format!("{workspace}.py");
                    let command = format!("python3 /workspaces/{workspace}/{basename}");
                    replace_entry(settings, "SessionStart", &basename, &command);
                })
                .unwrap();
            });
        }
    });

    let bytes = std::fs::read(path).unwrap();
    let settings: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(settings["permissions"]["allow"][0], "Read");
    let commands = settings["hooks"]["SessionStart"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["hooks"][0]["command"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        commands,
        std::collections::BTreeSet::from([
            "python3 /workspaces/family/family.py",
            "python3 /workspaces/work/work.py",
        ])
    );
}

#[test]
fn failed_atomic_hook_replacement_preserves_original_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hooks.json");
    let original = br#"{"permissions":{"allow":["Read"]}}"#;
    std::fs::write(&path, original).unwrap();
    let blocked_temporary = temp.path().join("blocked-temporary");
    std::fs::create_dir(&blocked_temporary).unwrap();

    let result = update_json_file_with_temporary(&path, &blocked_temporary, |settings| {
        settings["changed"] = json!(true);
    });

    assert!(result.is_err());
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn atomic_hook_replacement_preserves_an_existing_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("rendered-hooks.json");
    let link = temp.path().join("hooks.json");
    std::fs::write(&target, br#"{"before":true}"#).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    update_json_file(&link, |settings| {
        settings["after"] = json!(true);
    })
    .unwrap();

    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let updated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();
    assert_eq!(updated["before"], true);
    assert_eq!(updated["after"], true);
}

#[test]
fn install_adds_an_idempotent_opencode_brain_plugin() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("family");
    let home = temp.path().join("home");

    install_for_home(&root, &home).unwrap();

    let plugin = root.join(".opencode/plugins/brain.js");
    let source = std::fs::read_to_string(&plugin).unwrap();
    assert!(source.contains("session.created"));
    assert!(source.contains("session.idle"));

    install_for_home(&root, &home).unwrap();
    assert_eq!(std::fs::read_to_string(plugin).unwrap(), source);
}
