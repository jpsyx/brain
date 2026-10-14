use super::fixture::{WorkspaceFixture, needles};
use crate::tasks::removal::{
    apply, model::Options, operation_status, operations::Phase, preview, resume,
};

fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

fn configured() -> (
    WorkspaceFixture,
    crate::workspace::CommandContext,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let f = WorkspaceFixture::new();
    super::integration_config::registry(&f, serde_json::json!({}));
    let context = f.command();
    let directory = f.temporary.path().join("agendas");
    std::fs::create_dir(&directory).unwrap();
    crate::env::set(&context, "agenda_markdown_dir", directory.to_str().unwrap()).unwrap();
    crate::settings::set(
        &context.workspace,
        "agenda_dir",
        directory.to_str().unwrap(),
    )
    .unwrap();
    crate::env::set(&context, "markdown_to_pdf_path", "/nonexistent/renderer").unwrap();
    let markdown = directory.join(format!("{}.md", today()));
    let pdf = directory.join(format!("agenda-{}.pdf", today()));
    std::fs::write(&markdown, "## ❗ MIT\n\n- [ ] ❗ **T17** Send proposal\n").unwrap();
    (f, context, markdown, pdf)
}

#[test]
fn removal_agenda_warning_is_committed_persisted_and_complete_resume_does_not_refresh() {
    let (f, context, markdown, pdf) = configured();
    f.project("projects/example");
    std::fs::write(&pdf, b"old printable").unwrap();
    let plan = preview(&context, &needles(&["T17"]), Options::default(), today()).unwrap();
    let receipt = apply(&context, &plan, &plan.fingerprint, today()).unwrap();
    assert!(receipt.committed);
    assert_eq!(
        receipt.updated,
        ["projects/example/.METADATA.json", "tasks/tasks.csv"]
    );
    assert_eq!(receipt.warnings.len(), 1, "{receipt:?}");
    assert_eq!(receipt.warnings[0].code, "agenda_refresh_failed");
    assert_eq!(receipt.warnings[0].details["stage"], "pdf_render");
    assert!(
        !std::fs::read_to_string(&markdown)
            .unwrap()
            .contains("**T17**")
    );
    assert_eq!(std::fs::read(&pdf).unwrap(), b"old printable");
    let directory = context.workspace.paths().task_removals_dir();
    let commit = directory.join(format!("{}.commit.json", receipt.operation_id));
    let immutable = std::fs::read(&commit).unwrap();
    let record: serde_json::Value = serde_json::from_slice(&immutable).unwrap();
    assert_eq!(record["receipt"]["warnings"], serde_json::json!([]));
    let progress = directory.join(format!("{}.json", receipt.operation_id));
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&progress).unwrap()).unwrap();
    assert_eq!(
        saved["receipt"]["warnings"][0]["details"]["stage"],
        "pdf_render"
    );
    let status = operation_status(&context, &receipt.operation_id).unwrap();
    assert_eq!(status.phase, Phase::Complete);
    assert_eq!(status.receipt.as_ref(), Some(&receipt));
    std::fs::write(&markdown, "later agenda").unwrap();
    std::fs::write(&pdf, b"later printable").unwrap();
    assert_eq!(resume(&context, &receipt.operation_id).unwrap(), receipt);
    assert_eq!(std::fs::read_to_string(markdown).unwrap(), "later agenda");
    assert_eq!(std::fs::read(pdf).unwrap(), b"later printable");
    assert_eq!(std::fs::read(commit).unwrap(), immutable);
}

#[test]
fn removal_agenda_executor_applies_saved_successor_batch_without_deleted_snapshot_rows() {
    let (f, context, markdown, _) = configured();
    f.write("tasks/tasks.csv", b"task_id,task_name,status,task_type,last_touched\nT1,Read spec (1/4),not_started,mit,\nT2,Read spec (2/4),not_started,,\nT3,Read spec (3/4),not_started,,\n");
    std::fs::write(&markdown, "## ❗ MIT\n\n- [ ] ❗ **T1** Read spec (1/4)\n- [ ] ❗ **T2** Read spec (2/4)\n\n## Suggested order\n\n1. [ ] 09:00 | **T2** Read spec (2/4)\n2. [ ] 10:00 | **T1** Read spec (1/4)\n").unwrap();
    let plan = preview(
        &context,
        &needles(&["T1", "T2"]),
        Options::default(),
        today(),
    )
    .unwrap();
    assert_eq!(plan.successors.get("T2").map(String::as_str), Some("T3"));
    let before = plan
        .edits
        .iter()
        .find(|edit| edit.relative == std::path::Path::new("tasks/tasks.csv"))
        .unwrap()
        .before
        .as_ref()
        .unwrap();
    assert!(String::from_utf8_lossy(before).contains("T2,Read spec (2/4)"));
    let receipt = apply(&context, &plan, &plan.fingerprint, today()).unwrap();
    assert!(
        receipt.committed && receipt.warnings.is_empty(),
        "{receipt:?}"
    );
    let text = std::fs::read_to_string(markdown).unwrap();
    assert_eq!(text.matches("**T3**").count(), 2, "{text}");
    assert!(
        !text.contains("**T1**") && !text.contains("**T2**"),
        "{text}"
    );
    assert!(text.contains("1. [ ] 09:00 | **T3**"), "{text}");
    assert!(
        !std::fs::read_to_string(context.workspace.root().join("tasks/tasks.csv"))
            .unwrap()
            .contains("T2,")
    );
    assert!(
        String::from_utf8_lossy(
            plan.edits
                .iter()
                .find(|edit| edit.relative == std::path::Path::new("tasks/tasks.csv"))
                .unwrap()
                .before
                .as_ref()
                .unwrap()
        )
        .contains("T2,Read spec (2/4)")
    );
}

#[test]
fn removal_local_committed_resume_retries_printable_even_if_markdown_already_refreshed() {
    use std::os::unix::fs::PermissionsExt;
    let (_f, context, markdown, pdf) = configured();
    let plan = preview(&context, &needles(&["T17"]), Options::default(), today()).unwrap();
    let receipt = apply(&context, &plan, &plan.fingerprint, today()).unwrap();
    let refreshed = std::fs::read(&markdown).unwrap();
    std::fs::write(&pdf, b"stale printable").unwrap();
    let renderer = pdf.parent().unwrap().join("renderer");
    std::fs::write(&renderer, "#!/bin/sh\ncp \"$1\" \"$3\"\n").unwrap();
    std::fs::set_permissions(&renderer, std::fs::Permissions::from_mode(0o755)).unwrap();
    crate::env::set(&context, "markdown_to_pdf_path", renderer.to_str().unwrap()).unwrap();
    let directory = context.workspace.paths().task_removals_dir();
    let commit = directory.join(format!("{}.commit.json", receipt.operation_id));
    let immutable = std::fs::read(&commit).unwrap();
    let progress = directory.join(format!("{}.json", receipt.operation_id));
    let mut operation: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&progress).unwrap()).unwrap();
    operation["phase"] = serde_json::json!("local_committed");
    std::fs::write(&progress, serde_json::to_vec(&operation).unwrap()).unwrap();
    assert_eq!(resume(&context, &receipt.operation_id).unwrap(), receipt);
    assert_eq!(std::fs::read(&pdf).unwrap(), refreshed);
    assert_eq!(std::fs::read(markdown).unwrap(), refreshed);
    assert_eq!(std::fs::read(commit).unwrap(), immutable);
    assert_eq!(
        operation_status(&context, &receipt.operation_id)
            .unwrap()
            .phase,
        Phase::Complete
    );
}
