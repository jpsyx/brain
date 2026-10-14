use super::*;
use crate::workspace::{
    CommandContext, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName,
};
use clap::Parser;
#[test]
fn removal_shell_reads_current_date_again_at_apply() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    let bytes = b"task_id,task_name\nT1,First\n";
    std::fs::write(root.join("tasks/tasks.csv"), bytes).unwrap();
    std::fs::write(root.join("tasks/habits.csv"), b"task_id,task_name\n").unwrap();
    let workspace = WorkspaceContext::new(
        temp.path(),
        WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
        WorkspaceName::parse("example").unwrap(),
        &root,
        "member",
        temp.path(),
    )
    .unwrap();
    let context = CommandContext::for_test(
        std::sync::Arc::new(workspace),
        RegistryStore::from_path(temp.path().join("registry.json")),
        "member",
    );
    let cli = crate::tasks::cli::Cli::try_parse_from(["tasks", "remove", "T1", "--yes", "--json"])
        .unwrap();
    let Some(crate::tasks::cli::Command::Remove(args)) = cli.command else {
        panic!("remove")
    };
    let first = chrono::NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
    let second = first.succ_opt().unwrap();
    let mut dates = [first, second].into_iter();
    assert_eq!(
        targets_with_clock(&context, &args, || dates.next().expect("fresh clock read"))
            .unwrap_err()
            .code,
        "stale_plan"
    );
    assert_eq!(std::fs::read(root.join("tasks/tasks.csv")).unwrap(), bytes);
    assert!(dates.next().is_none());
}
#[test]
fn removal_mode_rejects_all_inherited_selection_filters() {
    for flag in [
        "--past-due",
        "--mit",
        "--stale",
        "--no-due",
        "--blocked",
        "--include-done",
        "--include-deferred",
    ] {
        let cli = crate::tasks::cli::Cli::try_parse_from(["tasks", "remove", "T1", flag]).unwrap();
        let Some(crate::tasks::cli::Command::Remove(args)) = cli.command else {
            panic!("remove")
        };
        assert_eq!(
            validate_mode(&args, &cli.filters, false, &[])
                .unwrap_err()
                .code,
            "removal_filters_not_supported"
        );
    }
    for flag in [
        "--hard-deadline",
        "--status",
        "--priority",
        "--type",
        "--project",
        "--energy",
        "--context",
        "--assigned-to",
        "--linear-issue",
        "--search",
    ] {
        let value = if flag == "--hard-deadline" {
            "false"
        } else {
            "sample"
        };
        let cli =
            crate::tasks::cli::Cli::try_parse_from(["tasks", flag, value, "remove", "T1"]).unwrap();
        let Some(crate::tasks::cli::Command::Remove(args)) = cli.command else {
            panic!("remove")
        };
        assert_eq!(
            validate_mode(&args, &cli.filters, false, &[])
                .unwrap_err()
                .code,
            "removal_filters_not_supported"
        );
    }
}
#[test]
fn removal_classifies_integration_validation_and_active_ownership_correctly() {
    for (code, expected) in [
        ("invalid_removal_integration", 2),
        ("operation_active", 1),
        ("external_pending", 1),
        ("snapshot_failed", 1),
        ("snapshot_read_failed", 1),
        ("invalid_workspace_path", 2),
        ("recovery_required", 1),
        ("ambiguous_chunk_family", 2),
        ("missing_last_touched_column", 2),
    ] {
        assert_eq!(
            report::exit_code(&Problem::new(code, "failure", json!({}))),
            expected,
            "{code}"
        );
    }
}
#[test]
fn removal_error_report_preserves_safe_operation_recovery_identity() {
    let error = Problem::new(
        "external_pending",
        "external outcome needs reconciliation",
        json!({"operation_id": "operation-1", "action_id": "action-1", "stage": "cancel", "cause": "timeout"}),
    );
    let human = report::human(&report::error(&error), crate::theme::Theme::dark(false));
    for value in ["operation-1", "action-1", "cancel", "timeout"] {
        assert!(human.contains(value), "{human}");
    }
}
