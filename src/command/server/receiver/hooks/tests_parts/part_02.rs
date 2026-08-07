
#[test]
fn merge_is_idempotent_and_preserves_other_settings() {
    let mut settings = json!({"permissions": {"allow": ["Read"]}});
    replace_entry(
        &mut settings,
        "SessionStart",
        &["session.py"],
        "/tmp/session.py",
    );
    replace_entry(
        &mut settings,
        "SessionStart",
        &["session.py"],
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
                    replace_entry(settings, "SessionStart", &[&basename], &command);
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
fn merge_removes_legacy_and_generic_brain_hooks_but_preserves_unrelated_hooks() {
    let mut settings = json!({
        "hooks": {
            "SessionStart": [
                {"hooks": [{"type": "command", "command": "python3 /old/claude_session_start_hook.py"}]},
                {"hooks": [{"type": "command", "command": "python3 /old/agent_session_start_hook.py"}]},
                {"hooks": [{"type": "command", "command": "python3 /keep/unrelated.py"}]}
            ]
        }
    });

    replace_entry(
        &mut settings,
        "SessionStart",
        &["claude_session_start_hook.py", "agent_session_start_hook.py"],
        "python3 .claude/brain-hooks/agent_session_start_hook.py",
    );

    let commands = settings["hooks"]["SessionStart"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|entry| entry["hooks"].as_array().unwrap())
        .map(|hook| hook["command"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        commands,
        vec![
            "python3 /keep/unrelated.py",
            "python3 .claude/brain-hooks/agent_session_start_hook.py",
        ]
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

#[test]
fn workspace_static_artifact_preserves_an_in_workspace_symlink_idempotently() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("family");
    let home = temp.path().join("home");
    let target = root.join("rendered/brain.js");
    let plugin = root.join(".opencode/plugins/brain.js");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
    std::fs::write(&target, "old\n").unwrap();
    std::os::unix::fs::symlink(&target, &plugin).unwrap();

    install_for_home(&root, &home).unwrap();
    let installed = std::fs::read_to_string(&target).unwrap();
    install_for_home(&root, &home).unwrap();

    assert!(installed.contains("session.created"));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), installed);
    assert!(
        std::fs::symlink_metadata(plugin)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn workspace_static_artifact_rejects_a_leaf_symlink_outside_the_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("family");
    let home = temp.path().join("home");
    let outside = temp.path().join("outside.js");
    let plugin = root.join(".opencode/plugins/brain.js");
    std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
    std::fs::write(&outside, "outside\n").unwrap();
    std::os::unix::fs::symlink(&outside, &plugin).unwrap();

    let result = install_for_home(&root, &home);

    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "outside\n");
    assert!(
        std::fs::symlink_metadata(plugin)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn workspace_static_artifact_rejects_a_parent_symlink_outside_the_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("family");
    let home = temp.path().join("home");
    let outside = temp.path().join("outside-plugins");
    let plugins = root.join(".opencode/plugins");
    std::fs::create_dir_all(plugins.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("brain.js"), "outside\n").unwrap();
    std::os::unix::fs::symlink(&outside, &plugins).unwrap();

    let result = install_for_home(&root, &home);

    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(outside.join("brain.js")).unwrap(),
        "outside\n"
    );
    assert!(
        std::fs::symlink_metadata(plugins)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
