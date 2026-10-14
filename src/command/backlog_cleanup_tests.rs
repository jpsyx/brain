use super::{run_dedupe, run_purge};
use crate::{
    cli::BacklogMaintenanceArgs,
    tasks::complete::{field, read_csv},
    workspace::{CommandContext, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName},
};
fn context(tmp: &tempfile::TempDir, csv: &str) -> CommandContext {
    let root = tmp.path().join("workspace");
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(root.join("tasks/tasks.csv"), csv).unwrap();
    let workspace = WorkspaceContext::new(
        tmp.path(),
        WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
        WorkspaceName::parse("example").unwrap(),
        &root,
        "member",
        tmp.path(),
    )
    .unwrap();
    CommandContext::for_test(
        std::sync::Arc::new(workspace),
        RegistryStore::from_path(tmp.path().join("env.json")),
        "member",
    )
}
fn args() -> BacklogMaintenanceArgs {
    BacklogMaintenanceArgs {
        dry_run: false,
        report: false,
    }
}
#[test]
fn backlog_removal_cleanup_purge_rewires_the_active_dependent() {
    let tmp = tempfile::tempdir().unwrap();
    let context = context(
        &tmp,
        "task_id,task_name,status,backlogged_date,blocked_by,last_touched\nT1,Expired,backlog,2000-01-01,T3,\nT2,Active,not_started,,T1,\nT3,Prerequisite,not_started,,,\n",
    );
    run_purge(&context, &args()).unwrap();
    let csv = read_csv(&context.workspace.root().join("tasks/tasks.csv")).unwrap();
    assert_eq!(field(&csv.rows[0], "task_id"), "T2");
    assert_eq!(field(&csv.rows[0], "blocked_by"), "T3");
}
#[test]
fn backlog_removal_cleanup_dedupe_redirects_to_the_survivor_and_keeps_issue() {
    let tmp = tempfile::tempdir().unwrap();
    let context = context(
        &tmp,
        "task_id,task_name,status,backlogged_date,created_date,blocked_by,see_also,last_touched,linear_issue\nT1,Write plan,backlog,2020-01-01,,,,,EX-1\nT2,Write plan,not_started,,2020-02-01,,,,EX-2\nT3,Review,not_started,,,T1,T1,,\n",
    );
    run_dedupe(&context, &args()).unwrap();
    let csv = read_csv(&context.workspace.root().join("tasks/tasks.csv")).unwrap();
    assert_eq!(field(&csv.rows[1], "blocked_by"), "T2");
    assert_eq!(field(&csv.rows[1], "see_also"), "T2");
    assert_eq!(field(&csv.rows[0], "linear_issue"), "EX-2");
}
