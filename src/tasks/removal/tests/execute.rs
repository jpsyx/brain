use super::fixture::{WorkspaceFixture, needles, today};
use crate::tasks::removal::{apply, model::Options, preview};

#[test]
fn stale_removal_plan_rejects_changed_notes_and_reused_display_id() {
    for (before, after) in [
        (
            "task_id,task_name,notes\nT17,Send proposal,old\n",
            "task_id,task_name,notes\nT17,Send proposal,new\n",
        ),
        (
            "task_id,task_name,task_uuid\nT17,Send proposal,6fc925c4-b131-4e77-9f4d-fde190cad338\n",
            "task_id,task_name,task_uuid\nT17,Replacement,13746cfe-053a-4534-8851-462046c5df88\n",
        ),
    ] {
        let fixture = WorkspaceFixture::new();
        fixture.write("tasks/tasks.csv", before.as_bytes());
        let context = fixture.command();
        let plan = preview(&context, &needles(&["T17"]), Options::default(), today()).unwrap();
        fixture.write("tasks/tasks.csv", after.as_bytes());
        let error = apply(&context, &plan, &plan.fingerprint, today()).unwrap_err();
        assert_eq!(error.code, "stale_plan");
        assert_eq!(
            std::fs::read(context.workspace.root().join("tasks/tasks.csv")).unwrap(),
            after.as_bytes()
        );
        assert!(
            !context
                .workspace
                .paths()
                .cache_dir()
                .join("task-removals")
                .read_dir()
                .is_ok_and(|entries| entries.flatten().any(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json")))
        );
    }
}

#[test]
fn consented_removal_commits_and_retains_operation() {
    let fixture = WorkspaceFixture::new();
    let context = fixture.command();
    let plan = preview(&context, &needles(&["T17"]), Options::default(), today()).unwrap();
    apply(&context, &plan, &plan.fingerprint, today()).unwrap();
    assert_eq!(
        std::fs::read_to_string(context.workspace.root().join("tasks/tasks.csv"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert!(
        context
            .workspace
            .paths()
            .cache_dir()
            .join("task-removals")
            .read_dir()
            .unwrap()
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .ends_with(".commit.json"))
    );
}

#[test]
fn removal_preview_refuses_pending_journal_without_recovery_writes() {
    let fixture = WorkspaceFixture::new();
    let context = fixture.command();
    let cache = context.workspace.paths().cache_dir();
    std::fs::create_dir_all(cache).unwrap();
    let journal = cache.join("task-removal-transaction.json");
    std::fs::write(&journal, b"pending journal").unwrap();
    let error = preview(&context, &needles(&["T17"]), Options::default(), today()).unwrap_err();
    assert_eq!(error.code, "recovery_required");
    assert_eq!(std::fs::read(journal).unwrap(), b"pending journal");
}

#[test]
fn local_executor_never_skips_external_actions_or_unwired_policy() {
    let fixture = WorkspaceFixture::new();
    let context = fixture.command();
    let mut plan = preview(&context, &needles(&["T17"]), Options::default(), today()).unwrap();
    plan.external
        .push(crate::tasks::removal::model::ExternalAction {
            adapter: "tracker".into(),
            issue: "EX-1".into(),
            action_id: "cancel-EX-1".into(),
        });
    assert!(apply(&context, &plan, &plan.fingerprint, today()).is_err());
    fixture.write(".config/config.json", br#"{"task_removal_integration":{"adapter":"tracker","link_field":"linear_issue","on_remove":"cancel"}}"#);
    assert_eq!(
        preview(&context, &needles(&["T17"]), Options::default(), today())
            .unwrap_err()
            .code,
        "integration_unavailable"
    );
    assert!(
        std::fs::read_to_string(context.workspace.root().join("tasks/tasks.csv"))
            .unwrap()
            .contains("T17")
    );
}

#[test]
fn removal_diagnostics_preserve_error_class_without_sensitive_values() {
    let error: serde_json::Error = serde::de::Error::custom("invalid value credential-secret");
    let detail = crate::tasks::removal::execute::diagnostic(&error.into());
    assert!(detail.contains("Data"));
    assert!(!detail.contains("credential-secret"));
    let error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "private-task-notes");
    let detail = crate::tasks::removal::execute::diagnostic(&error.into());
    assert!(detail.contains("PermissionDenied"));
    assert!(!detail.contains("private-task-notes"));
}
