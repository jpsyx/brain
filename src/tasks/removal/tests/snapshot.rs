use super::fixture::{WorkspaceFixture, needles, today};
use crate::tasks::removal::fingerprint::digest;
use crate::tasks::removal::model::Options;
use crate::tasks::removal::snapshot::Snapshot;

#[test]
fn snapshot_preview_preserves_all_before_images_and_legacy_identity() {
    let fixture = WorkspaceFixture::new();
    fixture.project("projects/work__proposal");
    fixture.project("archive/projects/work__meeting");
    fixture.write("tasks/.tasks_next_id", b"91\n");
    fixture.write("tasks/.habits_next_id", b"27\n");
    fixture.write("tasks/SCHEMA.json", b"{\"legacy\":true}\n");
    let paths = [
        "tasks/tasks.csv",
        "tasks/habits.csv",
        "tasks/.tasks_next_id",
        "tasks/.habits_next_id",
        "tasks/SCHEMA.json",
        "projects/work__proposal/.METADATA.json",
        "archive/projects/work__meeting/.METADATA.json",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|p| std::fs::read(fixture.workspace.root().join(p)).unwrap())
        .collect();
    let snapshot = Snapshot::read(&fixture.context(), today()).unwrap();
    let selected = snapshot
        .selected_identities(&needles(&["T17", "proposal"]), &Options::default())
        .unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].task_uuid.to_string(),
        "ed911f60-f37a-5fd0-b6e5-acbd58f66836"
    );
    assert_eq!(snapshot.projects.len(), 2);
    assert_eq!(snapshot.tasks.before.as_deref(), Some(before[0].as_slice()));
    for (path, bytes) in paths.iter().zip(before) {
        assert_eq!(
            std::fs::read(fixture.workspace.root().join(path)).unwrap(),
            bytes
        );
    }
}

#[test]
fn snapshot_missing_project_warns_and_malformed_metadata_errors() {
    let fixture = WorkspaceFixture::new();
    fixture.write(
        "tasks/tasks.csv",
        b"task_id,task_name,project\nT17,Send proposal,work__missing\n",
    );
    let snapshot = Snapshot::read(&fixture.context(), today()).unwrap();
    assert_eq!(snapshot.warnings[0].code, "missing_project");
    fixture.write("projects/work__bad/.METADATA.json", b"{bad");
    assert_eq!(
        Snapshot::read(&fixture.context(), today())
            .unwrap_err()
            .code,
        "invalid_project_metadata"
    );
}

#[test]
fn snapshot_rejects_malformed_metadata_shape_and_non_file_inputs() {
    let fixture = WorkspaceFixture::new();
    fixture.project("archive/projects/work__bad");
    let path = fixture
        .workspace
        .root()
        .join("archive/projects/work__bad/.METADATA.json");
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    metadata["tasks"] = serde_json::json!("T17");
    std::fs::write(&path, serde_json::to_vec(&metadata).unwrap()).unwrap();
    assert_eq!(
        Snapshot::read(&fixture.context(), today())
            .unwrap_err()
            .code,
        "invalid_project_metadata"
    );
    std::fs::remove_file(path).unwrap();
    let csv_path = fixture.workspace.root().join("tasks/tasks.csv");
    std::fs::remove_file(&csv_path).unwrap();
    std::fs::create_dir(&csv_path).unwrap();
    assert_eq!(
        Snapshot::read(&fixture.context(), today())
            .unwrap_err()
            .code,
        "invalid_workspace_path"
    );
    std::fs::remove_dir(csv_path).unwrap();
    fixture.write("projects", b"not a directory");
    assert_eq!(
        Snapshot::read(&fixture.context(), today())
            .unwrap_err()
            .code,
        "invalid_workspace_path"
    );
}

#[test]
fn snapshot_preserves_persisted_identity_and_rejects_legacy_collisions() {
    let fixture = WorkspaceFixture::new();
    fixture.write(
        "tasks/tasks.csv",
        b"task_id,task_name,task_uuid\nT17,Send proposal,8F4FF482-4D40-4A2D-91B1-73CA9F1BFAD4\n",
    );
    let snapshot = Snapshot::read(&fixture.context(), today()).unwrap();
    let identities = snapshot
        .selected_identities(&needles(&["T17"]), &Options::default())
        .unwrap();
    assert_eq!(
        identities[0].task_uuid.to_string(),
        "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4"
    );
    fixture.write("tasks/tasks.csv", b"task_id,task_name,task_uuid\nT17,Send proposal,\nT18,Other,ed911f60-f37a-5fd0-b6e5-acbd58f66836\n");
    assert_eq!(
        Snapshot::read(&fixture.context(), today())
            .unwrap_err()
            .code,
        "duplicate_task_uuid"
    );
}

#[test]
fn snapshot_validates_all_rows_not_only_selected_targets() {
    let fixture = WorkspaceFixture::new();
    for (bytes, code) in [
        (
            "task_id,task_name,task_uuid\nT17,Send proposal,\nT99,Other,invalid\n",
            "invalid_task_uuid",
        ),
        (
            "task_id,task_name\nT17,Send proposal\nT017,Other\n",
            "duplicate_task_id",
        ),
        ("task_id,task_name\nH17,Wrong kind\n", "invalid_task_id"),
    ] {
        fixture.write("tasks/tasks.csv", bytes.as_bytes());
        assert_eq!(
            Snapshot::read(&fixture.context(), today())
                .unwrap_err()
                .code,
            code
        );
    }
    let uuid = "8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4";
    fixture.write(
        "tasks/tasks.csv",
        format!("task_id,task_name,task_uuid\nT17,Send proposal,{uuid}\n").as_bytes(),
    );
    fixture.write(
        "tasks/habits.csv",
        format!("task_id,task_name,task_uuid\nH2,Walk,{uuid}\n").as_bytes(),
    );
    assert_eq!(
        Snapshot::read(&fixture.context(), today())
            .unwrap_err()
            .code,
        "duplicate_task_uuid"
    );
}

#[test]
fn removal_fingerprint_binds_semantics_and_preserves_absence() {
    let fixture = WorkspaceFixture::new();
    let snapshot = Snapshot::read(&fixture.context(), today()).unwrap();
    let options = Options::default();
    let targets = snapshot
        .selected_identities(&needles(&["T17"]), &options)
        .unwrap();
    let canonical = snapshot.canonical(&options, &targets);
    let original = digest(&canonical).unwrap();
    for field in ["workspace_uuid", "actor_id", "execution_date"] {
        let mut changed = snapshot.clone();
        match field {
            "workspace_uuid" => changed.workspace_uuid = "different-workspace".into(),
            "actor_id" => changed.actor_id = "other-member".into(),
            _ => changed.execution_date = "2026-10-15".into(),
        }
        assert_ne!(
            original,
            digest(&changed.canonical(&options, &targets)).unwrap(),
            "{field}"
        );
    }
    let actor = crate::actor::test_actor("other-member");
    let mut context = fixture.context();
    context.actor = &actor;
    let other_actor = Snapshot::read(&context, today()).unwrap();
    assert_ne!(
        original,
        digest(&other_actor.canonical(&options, &targets)).unwrap()
    );
    let tomorrow = Snapshot::read(&fixture.context(), today().succ_opt().unwrap()).unwrap();
    assert_ne!(
        original,
        digest(&tomorrow.canonical(&options, &targets)).unwrap()
    );
    let changed_options = Options {
        linked_issue: crate::tasks::removal::model::LinkedIssue::Keep,
        ..options.clone()
    };
    assert_ne!(
        original,
        digest(&snapshot.canonical(&changed_options, &targets)).unwrap()
    );
    fixture.write(
        "tasks/tasks.csv",
        b"task_id,task_name,project\nT17,Send revised proposal,\n",
    );
    let changed = Snapshot::read(&fixture.context(), today()).unwrap();
    assert_ne!(
        original,
        digest(&changed.canonical(&options, &targets)).unwrap()
    );
    fixture.project("projects/work__new");
    let metadata_changed = Snapshot::read(&fixture.context(), today()).unwrap();
    assert_ne!(
        digest(&changed.canonical(&options, &targets)).unwrap(),
        digest(&metadata_changed.canonical(&options, &targets)).unwrap()
    );
    let path = "projects/work__new/.METADATA.json";
    let mut bytes = std::fs::read(fixture.workspace.root().join(path)).unwrap();
    bytes.push(b'\n');
    fixture.write(path, &bytes);
    let edited = Snapshot::read(&fixture.context(), today()).unwrap();
    assert_ne!(
        digest(&metadata_changed.canonical(&options, &targets)).unwrap(),
        digest(&edited.canonical(&options, &targets)).unwrap()
    );
    std::fs::remove_file(fixture.workspace.root().join("tasks/habits.csv")).unwrap();
    let absent = Snapshot::read(&fixture.context(), today()).unwrap();
    fixture.write("tasks/habits.csv", b"");
    let empty = Snapshot::read(&fixture.context(), today()).unwrap();
    assert_ne!(
        digest(&absent.canonical(&options, &targets)).unwrap(),
        digest(&empty.canonical(&options, &targets)).unwrap()
    );
}

#[test]
fn removal_fingerprint_canonicalizes_targets_without_presentation_labels() {
    let fixture = WorkspaceFixture::new();
    fixture.write(
        "tasks/tasks.csv",
        b"task_id,task_name\nT17,Send proposal\nT18,Book meeting\n",
    );
    let snapshot = Snapshot::read(&fixture.context(), today()).unwrap();
    let options = Options::default();
    let targets = snapshot
        .selected_identities(&needles(&["T17", "T18"]), &options)
        .unwrap();
    let both = digest(&snapshot.canonical(&options, &targets)).unwrap();
    let mut permuted = vec![targets[1].clone(), targets[0].clone(), targets[0].clone()];
    permuted[0].task_name = "presentation only".into();
    assert_eq!(
        both,
        digest(&snapshot.canonical(&options, &permuted)).unwrap()
    );
    assert_ne!(
        digest(&snapshot.canonical(&options, &targets[..1])).unwrap(),
        digest(&snapshot.canonical(&options, &targets[1..])).unwrap()
    );
    let aliases = snapshot
        .selected_identities(&needles(&["proposal", "t017", "17", "T18"]), &options)
        .unwrap();
    assert_eq!(
        both,
        digest(&snapshot.canonical(&options, &aliases)).unwrap()
    );
}

#[test]
fn snapshot_owned_read_verifies_owner_and_waits_for_coherent_generation() {
    use crate::tasks::store_lock::TaskStoreOwner;
    let fixture = WorkspaceFixture::new();
    let other = WorkspaceFixture::new();
    let wrong_owner = TaskStoreOwner::acquire(&other.workspace).unwrap();
    assert_eq!(
        Snapshot::read_owned(&fixture.context(), today(), &wrong_owner)
            .unwrap_err()
            .code,
        "invalid_task_store_owner"
    );
    let owner = TaskStoreOwner::acquire(&fixture.workspace).unwrap();
    let workspace = fixture.workspace.clone();
    let (start_tx, start_rx) = std::sync::mpsc::channel();
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let actor = crate::actor::test_actor("member");
        let integration = crate::tasks::removal::model::IntegrationSnapshot::default();
        start_tx.send(()).unwrap();
        result_tx
            .send(Snapshot::read(
                &crate::tasks::removal::snapshot::SnapshotContext {
                    workspace: &workspace,
                    actor: &actor,
                    integration: &integration,
                },
                today(),
            ))
            .unwrap();
    });
    start_rx.recv().unwrap();
    assert!(
        result_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err()
    );
    fixture.write(
        "tasks/tasks.csv",
        b"task_id,task_name\nT17,New generation\n",
    );
    fixture.write(
        "tasks/habits.csv",
        b"task_id,task_name\nH2,New habit generation\n",
    );
    drop(owner);
    let snapshot = result_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap();
    reader.join().unwrap();
    assert_eq!(snapshot.tasks.parsed.rows[0]["task_name"], "New generation");
    assert_eq!(
        snapshot.habits.parsed.rows[0]["task_name"],
        "New habit generation"
    );
}

#[test]
fn removal_fingerprint_never_captures_config_secrets() {
    let mut fixture = WorkspaceFixture::new();
    fixture.integration.adapter = Some(crate::tasks::removal::model::AdapterSnapshot {
        argv: vec!["fake-adapter".into(), "cancel".into()],
        environment_names: std::collections::BTreeSet::from(["API_TOKEN".into()]),
        timeout_ms: 500,
    });
    fixture.write(
        ".config/config.json",
        br#"{"enable_triage_habits":false,"api_token":"secret-before"}"#,
    );
    let first = Snapshot::read(&fixture.context(), today()).unwrap();
    fixture.write(
        ".config/config.json",
        br#"{"enable_triage_habits":false,"api_token":"secret-after"}"#,
    );
    let second = Snapshot::read(&fixture.context(), today()).unwrap();
    let options = Options::default();
    let a = first.canonical(&options, &[]);
    let b = second.canonical(&options, &[]);
    let serialized = serde_json::to_string(&a).unwrap();
    assert!(!serialized.contains("secret-before"));
    assert!(serialized.contains("API_TOKEN"));
    assert_eq!(digest(&a).unwrap(), digest(&b).unwrap());
    assert!(!first.managed_habits_enabled);
    fixture.integration.policy = Some(crate::tasks::removal::model::IntegrationPolicy {
        adapter: "tracker".into(),
        link_field: "linear_issue".into(),
        on_remove: crate::tasks::removal::model::OnRemove::Cancel,
    });
    let changed = Snapshot::read(&fixture.context(), today()).unwrap();
    assert_ne!(
        digest(&a).unwrap(),
        digest(&changed.canonical(&options, &[])).unwrap()
    );
    fixture.integration.adapter.as_mut().unwrap().timeout_ms = 750;
    let timeout_changed = Snapshot::read(&fixture.context(), today()).unwrap();
    assert_ne!(
        digest(&changed.canonical(&options, &[])).unwrap(),
        digest(&timeout_changed.canonical(&options, &[])).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn snapshot_rejects_csv_symlinks_and_does_not_follow_project_symlinks() {
    use std::os::unix::fs::symlink;
    let fixture = WorkspaceFixture::new();
    let outside = fixture.temporary.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join(".METADATA.json"), b"malformed").unwrap();
    std::fs::create_dir_all(fixture.workspace.root().join("projects")).unwrap();
    symlink(
        &outside,
        fixture.workspace.root().join("projects/work__outside"),
    )
    .unwrap();
    assert!(
        Snapshot::read(&fixture.context(), today())
            .unwrap()
            .projects
            .is_empty()
    );
    std::fs::remove_file(fixture.workspace.root().join("tasks/tasks.csv")).unwrap();
    symlink(
        outside.join(".METADATA.json"),
        fixture.workspace.root().join("tasks/tasks.csv"),
    )
    .unwrap();
    assert_eq!(
        Snapshot::read(&fixture.context(), today())
            .unwrap_err()
            .code,
        "invalid_workspace_path"
    );
}

#[test]
fn snapshot_metadata_io_failure_remains_an_operational_problem() {
    let fixture = WorkspaceFixture::new();
    let root = fixture.workspace.root();
    std::fs::rename(root.join("tasks"), root.join("saved-tasks")).unwrap();
    std::fs::write(root.join("tasks"), b"a file blocks metadata inspection").unwrap();
    let error = Snapshot::read(&fixture.context(), today()).unwrap_err();
    assert_eq!(error.code, "snapshot_read_failed");
    assert_eq!(
        std::fs::read(root.join("tasks")).unwrap(),
        b"a file blocks metadata inspection"
    );
}
