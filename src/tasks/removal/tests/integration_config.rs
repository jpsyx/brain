use super::fixture::{WorkspaceFixture, today};
use crate::tasks::removal::{model::Options, preview};
use serde_json::json;

fn policy(f: &WorkspaceFixture) {
    f.write(".config/config.json", &serde_json::to_vec(&json!({
        "task_removal_integration": {"adapter":"tracker", "link_field":"linear_issue", "on_remove":"cancel"}
    })).unwrap());
}

fn registry(f: &WorkspaceFixture, adapters: serde_json::Value) {
    let command = f.command();
    command
        .registry_store
        .replace(&crate::workspace::MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: f.workspace.name().clone(),
            env: serde_json::Map::new(),
            workspaces: std::collections::BTreeMap::from([(
                f.workspace.name().clone(),
                crate::workspace::WorkspaceRecord {
                    workspace_id: f.workspace.id(),
                    root: f.workspace.root().to_owned(),
                    aliases: std::collections::BTreeSet::default(),
                    local_user_id: "member".into(),
                    receiver_enabled: false,
                    env: serde_json::Map::from_iter([("task_removal_adapters".into(), adapters)]),
                },
            )]),
        })
        .unwrap();
}

fn linked(f: &WorkspaceFixture, shared: bool) {
    let extra = if shared {
        "T18,Survivor,active,TRACK-1\n"
    } else {
        ""
    };
    f.write(
        "tasks/tasks.csv",
        format!("task_id,task_name,status,linear_issue\nT17,Target,active,TRACK-1\n{extra}")
            .as_bytes(),
    );
}

#[test]
fn removal_integration_policy_absent_keeps_links_without_registry() {
    let f = WorkspaceFixture::new();
    linked(&f, false);
    let plan = preview(&f.command(), &["T17".into()], Options::default(), today()).unwrap();
    assert!(plan.external.is_empty());
    assert!(plan.blockers.is_empty());
}

#[test]
fn removal_integration_policy_missing_adapter_blocks_cancel() {
    let f = WorkspaceFixture::new();
    linked(&f, false);
    policy(&f);
    registry(&f, json!({}));
    let plan = preview(&f.command(), &["T17".into()], Options::default(), today()).unwrap();
    assert_eq!(plan.blockers[0].code, "adapter_unavailable");
}

#[test]
fn removal_integration_policy_shared_survivor_blocks_cancel() {
    let f = WorkspaceFixture::new();
    linked(&f, true);
    policy(&f);
    registry(
        &f,
        json!({"tracker":{"argv":["/bin/sh"],"env_names":[],"timeout_seconds":30}}),
    );
    let plan = preview(&f.command(), &["T17".into()], Options::default(), today()).unwrap();
    assert_eq!(plan.blockers[0].code, "shared_external_link");
}

#[test]
fn removal_integration_policy_setting_is_structured_strict_and_nullable() {
    let f = WorkspaceFixture::new();
    let value = r#"{"adapter":"tracker","link_field":"linear_issue","on_remove":"cancel"}"#;
    crate::settings::set(&f.workspace, "task_removal_integration", value).unwrap();
    let stored: serde_json::Value = serde_json::from_slice(
        &std::fs::read(f.workspace.root().join(".config/config.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(stored["task_removal_integration"]["adapter"], "tracker");
    for invalid in [
        r#"{"adapter":"tracker","link_field":"other","on_remove":"cancel"}"#,
        r#"{"adapter":"","link_field":"linear_issue","on_remove":"cancel"}"#,
        r#"{"adapter":"tracker","link_field":"linear_issue","on_remove":"delete"}"#,
        r#"{"adapter":"tracker","link_field":"linear_issue","on_remove":"keep","token":"secret"}"#,
    ] {
        assert!(crate::settings::set(&f.workspace, "task_removal_integration", invalid).is_err());
        f.write(
            ".config/config.json",
            format!("{{\"task_removal_integration\":{invalid}}}").as_bytes(),
        );
        assert!(preview(&f.command(), &["T17".into()], Options::default(), today()).is_err());
    }
    crate::settings::set(&f.workspace, "task_removal_integration", "null").unwrap();
    assert!(preview(&f.command(), &["T17".into()], Options::default(), today()).is_ok());
}

#[test]
fn removal_integration_policy_adapter_settings_validate_full_and_dotted_candidates() {
    let f = WorkspaceFixture::new();
    registry(&f, json!({}));
    let command = f.command();
    let valid = json!({"tracker":{"argv":["/bin/sh"],"env_names":["TOKEN"],"timeout_seconds":30}});
    crate::env::set(&command, "task_removal_adapters", &valid.to_string()).unwrap();
    crate::env::set(
        &command,
        "task_removal_adapters.tracker.timeout_seconds",
        "120",
    )
    .unwrap();
    let before = std::fs::read(command.registry_store.path()).unwrap();
    for (path, value) in [
        ("argv", "[]"),
        ("argv", "[\"relative\"]"),
        ("env_names", "[\"TOKEN\",\"TOKEN\"]"),
        ("env_names", "[\"bad-name\"]"),
        ("timeout_seconds", "0"),
        ("timeout_seconds", "121"),
        ("secret", "literal"),
    ] {
        assert!(
            crate::env::set(
                &command,
                &format!("task_removal_adapters.tracker.{path}"),
                value
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read(command.registry_store.path()).unwrap(),
            before
        );
    }
    assert!(crate::env::set(&command, "task_removal_adapters", "null").is_err());
}

#[test]
fn removal_integration_policy_preview_checks_metadata_without_launching() {
    use std::os::unix::fs::PermissionsExt;
    let f = WorkspaceFixture::new();
    linked(&f, false);
    policy(&f);
    let executable = f.temporary.path().join("adapter");
    let marker = f.temporary.path().join("launched");
    std::fs::write(
        &executable,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    registry(
        &f,
        json!({"tracker":{"argv":[executable],"env_names":["MISSING_FAKE_TOKEN"],"timeout_seconds":1}}),
    );
    let plan = preview(&f.command(), &["T17".into()], Options::default(), today()).unwrap();
    assert_eq!(plan.external.len(), 1);
    assert!(!marker.exists());
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o600)).unwrap();
    let plan = preview(&f.command(), &["T17".into()], Options::default(), today()).unwrap();
    assert_eq!(plan.blockers[0].code, "adapter_unavailable");
    assert!(!marker.exists());
    let keep = Options {
        linked_issue: crate::tasks::removal::model::LinkedIssue::Keep,
        ..Options::default()
    };
    assert!(
        preview(&f.command(), &["T17".into()], keep, today())
            .unwrap()
            .blockers
            .is_empty()
    );
}

#[test]
fn removal_integration_policy_provenance_fails_closed_on_uuid_change() {
    let f = WorkspaceFixture::new();
    policy(&f);
    registry(&f, json!({}));
    let command = f.command();
    let mut registry =
        crate::workspace::RegistryStore::load_from(command.registry_store.path()).unwrap();
    registry
        .workspaces
        .get_mut(f.workspace.name())
        .unwrap()
        .workspace_id = crate::workspace::WorkspaceId::new();
    command.registry_store.replace(&registry).unwrap();
    assert_eq!(
        preview(&command, &["T17".into()], Options::default(), today())
            .unwrap_err()
            .code,
        "invalid_removal_integration"
    );
}

#[test]
fn removal_integration_policy_configuration_stays_with_selected_record_after_default_change() {
    let f = WorkspaceFixture::new();
    linked(&f, false);
    policy(&f);
    registry(
        &f,
        json!({"tracker":{"argv":["/bin/sh"],"env_names":[],"timeout_seconds":30}}),
    );
    let command = f.command();
    let mut registry =
        crate::workspace::RegistryStore::load_from(command.registry_store.path()).unwrap();
    let other = crate::workspace::WorkspaceName::parse("other").unwrap();
    let mut record = registry.workspaces[f.workspace.name()].clone();
    record.workspace_id = crate::workspace::WorkspaceId::new();
    record.root = f.temporary.path().join("other");
    record.env.insert(
        "task_removal_adapters".into(),
        json!({"tracker":{"argv":["/missing/other"],"env_names":[],"timeout_seconds":2}}),
    );
    registry.workspaces.insert(other.clone(), record);
    registry.default_workspace = other;
    registry.env.insert(
        "task_removal_adapters".into(),
        json!("invalid-global-value"),
    );
    command.registry_store.replace(&registry).unwrap();
    let plan = preview(&command, &["T17".into()], Options::default(), today()).unwrap();
    assert!(plan.blockers.is_empty());
    assert_eq!(plan.external.len(), 1);
}

#[test]
fn removal_integration_policy_accepts_executable_symlink_and_missing_no_link_is_noop() {
    let f = WorkspaceFixture::new();
    policy(&f);
    let executable = f.temporary.path().join("symlink");
    std::os::unix::fs::symlink("/bin/sh", &executable).unwrap();
    registry(
        &f,
        json!({"tracker":{"argv":[executable],"env_names":[],"timeout_seconds":1}}),
    );
    linked(&f, false);
    let plan = preview(&f.command(), &["T17".into()], Options::default(), today()).unwrap();
    assert_eq!(plan.external.len(), 1);
    std::fs::remove_file(executable).unwrap();
    f.write(
        "tasks/tasks.csv",
        b"task_id,task_name,linear_issue\nT17,Unlinked,\n",
    );
    let plan = preview(&f.command(), &["T17".into()], Options::default(), today()).unwrap();
    assert!(plan.blockers.is_empty());
    assert!(plan.external.is_empty());
}
