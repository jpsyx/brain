use super::*;

#[test]
fn removal_rejects_unexpected_before_image_without_publishing() {
    let (_temporary, workspace, mut changes) = removal_fixture();
    changes[2].before = Some(b"stale".to_vec());
    let owner = TaskStoreOwner::acquire(&workspace).unwrap();
    assert!(replace_group(&workspace, &owner, TargetPolicy::TaskRemoval, &changes).is_err());
    assert_eq!(
        std::fs::read(&changes[0].path).unwrap(),
        changes[0].before.clone().unwrap()
    );
    assert!(!journal_path(&workspace, TargetPolicy::TaskRemoval).exists());
}

#[test]
fn malformed_removal_journals_are_rejected_before_any_target_is_touched() {
    for mutation in [
        "json",
        "version",
        "policy",
        "missing_policy",
        "uuid",
        "root",
        "absolute",
        "traversal",
        "outside_policy",
        "artifact",
        "duplicate",
    ] {
        let (_temporary, workspace, changes) = removal_fixture();
        let owner = TaskStoreOwner::acquire(&workspace).unwrap();
        crash_at(&workspace, &owner, &changes, TransactionStep::Install(0));
        let path = journal_path(&workspace, TargetPolicy::TaskRemoval);
        let mut journal: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(journal["schema_version"], 3);
        assert_eq!(journal["policy"], "task_removal");
        match mutation {
            "version" => journal["schema_version"] = 2.into(),
            "policy" => journal["policy"] = "managed_triage".into(),
            "missing_policy" => {
                journal.as_object_mut().unwrap().remove("policy");
            }
            "uuid" => journal["workspace_id"] = "11111111-1111-4111-8111-111111111111".into(),
            "root" => {
                journal["workspace_root"] =
                    workspace.root().join("other").display().to_string().into();
            }
            "absolute" => journal["entries"][0]["live"] = "/tasks/tasks.csv".into(),
            "traversal" => journal["entries"][0]["live"] = "tasks/../tasks/tasks.csv".into(),
            "outside_policy" => journal["entries"][0]["live"] = ".config/config.json".into(),
            "artifact" => journal["entries"][0]["backup"] = "tasks/forged.backup".into(),
            "duplicate" => {
                let first = journal["entries"][0].clone();
                journal["entries"].as_array_mut().unwrap().push(first);
            }
            "json" => {}
            _ => unreachable!(),
        }
        let bytes = if mutation == "json" {
            b"{".to_vec()
        } else {
            serde_json::to_vec(&journal).unwrap()
        };
        std::fs::write(&path, bytes).unwrap();
        assert!(recover_pending(&workspace, &owner).is_err(), "{mutation}");
        assert_old(&changes);
        assert!(path.exists());
    }
}

#[test]
fn removal_and_triage_policies_are_not_interchangeable() {
    let (_temporary, workspace, changes) = removal_fixture();
    let owner = TaskStoreOwner::acquire(&workspace).unwrap();
    assert!(replace_group(&workspace, &owner, TargetPolicy::ManagedTriage, &changes).is_err());
    let (_, _, triage) = fixture();
    let config = FileChange {
        path: workspace.root().join(".config/config.json"),
        ..triage[2].clone()
    };
    assert!(replace_group(&workspace, &owner, TargetPolicy::TaskRemoval, &[config]).is_err());
    assert_old(&changes);
}

#[cfg(unix)]
#[test]
fn removal_rejects_symlink_targets_ancestors_and_recovery_artifacts() {
    use std::os::unix::fs::symlink;
    for target in ["live", "ancestor", "backup", "staged"] {
        let (_temporary, workspace, changes) = removal_fixture();
        let outside = tempfile::tempdir().unwrap();
        let victim = outside.path().join("victim");
        std::fs::write(&victim, b"outside").unwrap();
        let owner = TaskStoreOwner::acquire(&workspace).unwrap();
        if matches!(target, "backup" | "staged") {
            crash_at(&workspace, &owner, &changes, TransactionStep::Install(0));
            let journal: serde_json::Value = serde_json::from_slice(
                &std::fs::read(journal_path(&workspace, TargetPolicy::TaskRemoval)).unwrap(),
            )
            .unwrap();
            let backup = workspace
                .root()
                .join(journal["entries"][0][target].as_str().unwrap());
            std::fs::remove_file(&backup).unwrap();
            symlink(&victim, &backup).unwrap();
            assert!(
                recover_pending(&workspace, &owner).is_err(),
                "{target} symlink accepted"
            );
            assert_old(&changes);
        } else {
            let path = &changes[2].path;
            std::fs::remove_file(path).unwrap();
            if target == "live" {
                symlink(&victim, path).unwrap();
            } else {
                std::fs::remove_dir(path.parent().unwrap()).unwrap();
                symlink(outside.path(), path.parent().unwrap()).unwrap();
            }
            assert!(
                replace_group(&workspace, &owner, TargetPolicy::TaskRemoval, &changes).is_err()
            );
        }
        assert_eq!(std::fs::read(&victim).unwrap(), b"outside");
    }
}

#[test]
fn empty_prepared_journal_is_rejected_without_panicking() {
    let (_temporary, workspace, changes) = removal_fixture();
    let owner = TaskStoreOwner::acquire(&workspace).unwrap();
    crash_at(&workspace, &owner, &changes, TransactionStep::Install(0));
    let path = journal_path(&workspace, TargetPolicy::TaskRemoval);
    let mut journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    journal["entries"] = serde_json::json!([]);
    std::fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
    assert!(recover_pending(&workspace, &owner).is_err());
    assert_old(&changes);
    assert!(path.exists());
}

#[test]
fn foreign_owner_cannot_replace_or_recover_selected_workspace() {
    let (_temporary, workspace, changes) = removal_fixture();
    let (_other_temporary, other, _) = fixture();
    let owner = TaskStoreOwner::acquire(&other).unwrap();
    assert!(replace_group(&workspace, &owner, TargetPolicy::TaskRemoval, &changes).is_err());
    assert!(recover_pending(&workspace, &owner).is_err());
    assert_old(&changes);
    assert!(!journal_path(&workspace, TargetPolicy::TaskRemoval).exists());
}

#[test]
fn removal_path_policy_accepts_only_csvs_and_project_reference_files() {
    for allowed in [
        "tasks/tasks.csv",
        "tasks/habits.csv",
        "projects/work__sample/.METADATA.json",
        "projects/work__sample/notes.md",
        "archive/projects/work__sample/.METADATA.json",
        "archive/projects/work__sample/notes.md",
    ] {
        assert!(
            removal_path_allowed(std::path::Path::new(allowed)),
            "{allowed}"
        );
    }
    for rejected in [
        "",
        "/tasks/tasks.csv",
        "tasks/../tasks/tasks.csv",
        ".config/config.json",
        "tasks/.tasks_next_id",
        "tasks/index.md",
        "projects/work__sample/private.md",
        "archive/resources/work__sample/notes.md",
    ] {
        assert!(
            !removal_path_allowed(std::path::Path::new(rejected)),
            "{rejected}"
        );
    }
}
