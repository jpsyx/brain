// Setup's task-schema preflight: what the remote's CSVs prove, not that they exist.

const CURRENT_TASKS: &str = "task_uuid,task_id,task_name,task_type,status,waiting_since,priority,due_date,hard_deadline,start_date,assigned_to,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue,system_key\n";
const CURRENT_HABITS: &str = "task_uuid,task_id,task_name,status,priority,due_date,hard_deadline,assigned_to,see_also,notes,project,energy_level,context,estimated_duration,ideal_time,recur_interval,recur_unit,created_date,completed_date,last_touched,system_key\n";

fn current_root(root: &std::path::Path) {
    let tasks = root.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::write(tasks.join("tasks.csv"), CURRENT_TASKS).unwrap();
    std::fs::write(tasks.join("habits.csv"), CURRENT_HABITS).unwrap();
    std::fs::write(
        tasks.join("SCHEMA.json"),
        crate::tasks::schema::CANONICAL_DOCUMENT,
    )
    .unwrap();
}

fn workspace_paths(base: &std::path::Path) -> crate::workspace::WorkspacePaths {
    crate::workspace::WorkspacePaths::new(base, crate::workspace::WorkspaceId::new())
}

/// The `~/family` dead end from the setup side: the remote held header-only
/// current CSVs and no schema document, and setup refused because CSV files
/// *existed*, calling current data legacy. Neither sync nor setup could run.
#[test]
fn current_remote_csvs_without_a_document_are_initialized_not_refused() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    current_root(&root);
    let published = std::cell::RefCell::new(Vec::<String>::new());

    let transitioned = super::prepare_current_schema_for_setup_with_transport(
        &workspace_paths(&temporary.path().join("home")),
        &root,
        None,
        crate::sync::csv_merge::RemoteCsvState::Current,
        |relative, _bytes| {
            published.borrow_mut().push(relative.to_owned());
            true
        },
    )
    .expect("a current remote missing only its document must be initialized");

    assert!(transitioned);
    assert!(
        published
            .into_inner()
            .iter()
            .any(|relative| relative.ends_with("SCHEMA.json")),
        "the schema document must be published"
    );
}

#[test]
fn genuinely_legacy_remote_csvs_are_still_refused() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    current_root(&root);
    let published = std::cell::RefCell::new(0_usize);

    let error = super::prepare_current_schema_for_setup_with_transport(
        &workspace_paths(&temporary.path().join("home")),
        &root,
        None,
        crate::sync::csv_merge::RemoteCsvState::Legacy,
        |_relative, _bytes| {
            *published.borrow_mut() += 1;
            true
        },
    )
    .expect_err("legacy remote rows must not be overwritten");

    assert!(error.to_string().contains("legacy"), "{error:#}");
    assert_eq!(published.into_inner(), 0);
}

#[test]
fn an_empty_remote_is_initialized_as_before() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("workspace");
    current_root(&root);

    let transitioned = super::prepare_current_schema_for_setup_with_transport(
        &workspace_paths(&temporary.path().join("home")),
        &root,
        None,
        crate::sync::csv_merge::RemoteCsvState::Absent,
        |_relative, _bytes| true,
    )
    .expect("an empty remote is the original initialization case");

    assert!(transitioned);
}
