use super::external::{calls, fixture, progress, rows, start};
use crate::tasks::removal::{abandon, operation_status, operations::Phase, resume};

#[test]
fn removal_external_crash_after_remote_success_queries_status_without_recancel() {
    let f = fixture(
        "if r['action']=='cancel':\n    (base/r['issue']).write_text('canceled')\n    sys.exit(1)\nif r['action']=='status': status='canceled' if (base/r['issue']).exists() else 'pending'",
    );
    let before = rows(&f);
    let error = start(&f).unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    assert_eq!(rows(&f), before);
    assert_eq!(progress(&f, id)["external"][0]["state"], "unknown");
    assert_eq!(
        abandon(&f.command(), id).unwrap_err().code,
        "external_pending"
    );
    assert_eq!(
        resume(&f.command(), id).unwrap_err().code,
        "external_pending"
    );
    assert!(resume(&f.command(), id).unwrap().committed);
    assert_eq!(calls(&f).matches("cancel EX-1\n").count(), 1);
    assert_eq!(calls(&f).matches("cancel EX-2\n").count(), 1);
}

#[test]
fn removal_external_already_canceled_is_success_and_complete_is_immutable() {
    let f = fixture("if r['action']=='cancel': status='already_canceled'");
    let receipt = start(&f).unwrap();
    let before_calls = calls(&f);
    f.write("tasks/tasks.csv", b"task_id,task_name\nT17,Replacement\n");
    assert_eq!(
        resume(&f.command(), &receipt.operation_id).unwrap(),
        receipt
    );
    assert_eq!(calls(&f), before_calls);
    assert!(rows(&f).contains("Replacement"));
    assert_eq!(
        abandon(&f.command(), &receipt.operation_id)
            .unwrap_err()
            .code,
        "already_committed"
    );
}

#[test]
fn removal_external_unknown_stays_unknown_after_status_error() {
    let f = fixture("if r['action']!='preflight': status='error'");
    let error = start(&f).unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    let before = rows(&f);
    let before_calls = calls(&f);
    let _ = operation_status(&f.command(), id).unwrap();
    let _ = operation_status(&f.command(), id).unwrap();
    assert_eq!(calls(&f), before_calls, "status only reads saved progress");
    assert_eq!(
        resume(&f.command(), id).unwrap_err().code,
        "external_pending"
    );
    assert_eq!(progress(&f, id)["external"][0]["state"], "unknown");
    assert_eq!(calls(&f).matches("cancel EX-1\n").count(), 1);
    assert_eq!(rows(&f), before);
    let saved = serde_json::to_string(&progress(&f, id)).unwrap();
    assert!(!saved.contains("provider diagnostic"));
}

#[test]
fn removal_external_preflight_all_before_cancel_and_abandon_preserves_rows() {
    let f = fixture("if r['action']=='preflight' and r['issue']=='EX-2': status='error'");
    let before = rows(&f);
    let error = start(&f).unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    assert_eq!(calls(&f), "preflight EX-1\npreflight EX-2\n");
    assert_eq!(abandon(&f.command(), id).unwrap().phase, Phase::Abandoned);
    assert_eq!(rows(&f), before);
    assert_eq!(
        resume(&f.command(), id).unwrap_err().code,
        "operation_abandoned"
    );
}

#[test]
fn removal_external_changed_adapter_needs_reconfirmation_without_invocation() {
    let f = fixture("if r['action']=='cancel': status='error'");
    let error = start(&f).unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    let before = calls(&f);
    crate::env::set(
        &f.command(),
        "task_removal_adapters.tracker.timeout_seconds",
        "3",
    )
    .unwrap();
    assert_eq!(resume(&f.command(), id).unwrap_err().code, "stale_plan");
    assert_eq!(
        operation_status(&f.command(), id).unwrap().phase,
        Phase::NeedsReconfirmation
    );
    assert_eq!(calls(&f), before);
}

#[test]
fn removal_external_actor_mismatch_blocks_resume_and_abandon() {
    let f = fixture("if r['action']=='cancel': status='error'");
    let error = start(&f).unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    let mut other = f.command();
    other.actor = crate::actor::test_actor("different");
    let before = calls(&f);
    assert_eq!(resume(&other, id).unwrap_err().code, "actor_mismatch");
    assert_eq!(abandon(&other, id).unwrap_err().code, "actor_mismatch");
    assert_eq!(calls(&f), before);
}

#[test]
fn removal_external_active_abandon_rejects_without_waiting_for_runner() {
    let f = fixture("if r['action']=='preflight': status='error'");
    let error = start(&f).unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    let owner =
        crate::tasks::removal::operations::OperationOwner::acquire(&f.workspace, id).unwrap();
    let holder = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        drop(owner);
    });
    let start = std::time::Instant::now();
    let result = abandon(&f.command(), id);
    let elapsed = start.elapsed();
    holder.join().unwrap();
    assert_eq!(result.unwrap_err().code, "operation_active");
    assert!(elapsed < std::time::Duration::from_millis(300));
    assert_eq!(progress(&f, id)["phase"], "prepared");
}

#[test]
fn removal_external_progress_identity_mismatch_fails_before_invocation() {
    let f = fixture(
        "if r['action']=='cancel': status='error'\nif r['action']=='status': status='pending'",
    );
    let error = start(&f).unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    let before = calls(&f);
    let mut saved = progress(&f, id);
    saved["external"][0]["issue"] = serde_json::json!("UNCONSENTED-1");
    std::fs::write(
        f.workspace
            .paths()
            .task_removals_dir()
            .join(format!("{id}.json")),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();
    assert_eq!(
        resume(&f.command(), id).unwrap_err().code,
        "operation_unavailable"
    );
    assert_eq!(calls(&f), before);
}

#[test]
fn removal_external_wrong_workspace_resume_has_no_adapter_or_local_effect() {
    let f = fixture("if r['action']=='cancel': status='error'");
    let error = start(&f).unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    let mut other = fixture("");
    other.workspace = crate::workspace::WorkspaceContext::new(
        other.temporary.path(),
        crate::workspace::WorkspaceId::new(),
        other.workspace.name().clone(),
        other.workspace.root(),
        "member",
        other.temporary.path(),
    )
    .unwrap();
    let directory = other.workspace.paths().task_removals_dir();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::copy(
        f.workspace
            .paths()
            .task_removals_dir()
            .join(format!("{id}.json")),
        directory.join(format!("{id}.json")),
    )
    .unwrap();
    let before = rows(&other);
    assert_eq!(
        resume(&other.command(), id).unwrap_err().code,
        "operation_unavailable"
    );
    assert!(calls(&other).is_empty());
    assert_eq!(rows(&other), before);
}

#[test]
fn removal_external_completed_provider_conflict_blocks_local_commit() {
    let f = fixture(
        "if r['action']=='preflight': status='error' # completed provider issue cannot be canceled",
    );
    let before = rows(&f);
    let error = start(&f).unwrap_err();
    assert_eq!(error.code, "external_pending");
    assert!(!calls(&f).contains("cancel"));
    assert_eq!(rows(&f), before);
}

#[test]
fn removal_external_deduplicates_selected_issues_before_invocation() {
    let f = fixture("");
    f.write(
        "tasks/tasks.csv",
        b"task_id,task_name,status,linear_issue\nT17,First,active,EX-1\nT18,Second,active,EX-1\n",
    );
    let receipt = start(&f).unwrap();
    assert_eq!(receipt.removed.len(), 2);
    assert_eq!(receipt.external.len(), 1);
    assert_eq!(calls(&f), "preflight EX-1\ncancel EX-1\n");
}

#[test]
fn removal_external_status_final_success_persists_external_complete_before_local_check() {
    let f = fixture("if r['action']=='cancel': sys.exit(1)");
    f.write(
        "tasks/tasks.csv",
        b"task_id,task_name,status,linear_issue\nT17,First,active,EX-1\nT18,Second,active,EX-1\n",
    );
    let error = start(&f).unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    let owner =
        crate::tasks::removal::operations::OperationOwner::acquire(&f.workspace, id).unwrap();
    let mut operation = operation_status(&f.command(), id).unwrap();
    crate::tasks::removal::external::reconcile(&f.command(), &owner, &mut operation).unwrap();
    assert_eq!(
        operation_status(&f.command(), id).unwrap().phase,
        Phase::ExternalComplete
    );
    assert!(rows(&f).contains("T17"));
}

#[test]
fn acceptance_partial_external_failure_resumes_once_with_committed_pdf_warning() {
    let f = fixture(
        "if r['action']=='cancel':\n    (base/r['issue']).write_text('canceled')\n    if r['issue']=='EX-2': sys.exit(1)\nif r['action']=='status': status='canceled' if (base/r['issue']).exists() else 'pending'",
    );
    let context = f.command();
    let date = chrono::Local::now().date_naive();
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
    let markdown = directory.join(format!("{date}.md"));
    let pdf = directory.join(format!("agenda-{date}.pdf"));
    std::fs::write(
        &markdown,
        "## ❗ MIT\n\n- [ ] ❗ **T17** First\n- [ ] ❗ **T18** Second\n",
    )
    .unwrap();
    std::fs::write(&pdf, b"old printable").unwrap();
    let before = rows(&f);
    let error = start(&f).unwrap_err();
    assert_eq!(error.code, "external_pending");
    let id = error.details["operation_id"].as_str().unwrap();
    assert_eq!(rows(&f), before);
    assert_eq!(progress(&f, id)["external"][0]["state"], "succeeded");
    let receipt = resume(&context, id).unwrap();
    assert!(receipt.committed);
    assert_eq!(receipt.removed.len(), 2);
    assert_eq!(rows(&f).lines().count(), 1);
    assert_eq!(calls(&f).matches("cancel EX-1\n").count(), 1);
    assert_eq!(calls(&f).matches("cancel EX-2\n").count(), 1);
    assert!(calls(&f).contains("status EX-2\n"));
    assert_eq!(receipt.warnings.len(), 1);
    assert_eq!(receipt.warnings[0].code, "agenda_refresh_failed");
    assert_eq!(receipt.warnings[0].details["stage"], "pdf_render");
    assert!(
        !std::fs::read_to_string(&markdown)
            .unwrap()
            .contains("**T17**")
    );
    assert!(
        !std::fs::read_to_string(&markdown)
            .unwrap()
            .contains("**T18**")
    );
    assert_eq!(std::fs::read(&pdf).unwrap(), b"old printable");
    let commit = f
        .workspace
        .paths()
        .task_removals_dir()
        .join(format!("{id}.commit.json"));
    let committed = std::fs::read(&commit).unwrap();
    let commit_count = || {
        std::fs::read_dir(f.workspace.paths().task_removals_dir())
            .unwrap()
            .map(Result::unwrap)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".commit.json")
            })
            .count()
    };
    assert_eq!(commit_count(), 1);
    assert_eq!(
        operation_status(&context, id).unwrap().receipt.as_ref(),
        Some(&receipt)
    );
    // Replay cannot publish again or overwrite canonical/derived edits made later.
    f.write(
        "tasks/tasks.csv",
        b"task_id,task_name\nT17,Later replacement\n",
    );
    std::fs::write(&markdown, "later agenda").unwrap();
    std::fs::write(&pdf, b"later printable").unwrap();
    let completed_calls = calls(&f);
    for _ in 0..2 {
        assert_eq!(resume(&context, id).unwrap(), receipt);
        assert_eq!(std::fs::read(&commit).unwrap(), committed);
        assert_eq!(commit_count(), 1);
        assert_eq!(calls(&f), completed_calls);
        assert_eq!(rows(&f), "task_id,task_name\nT17,Later replacement\n");
        assert_eq!(std::fs::read_to_string(&markdown).unwrap(), "later agenda");
        assert_eq!(std::fs::read(&pdf).unwrap(), b"later printable");
    }
}
