use super::{
    TriageAlertEvent, refresh_after_successful_startup_sync, should_check_daily_triage,
    triage_gate_resolved,
};

#[test]
fn palette_reenable_defers_until_refresh_then_uses_live_alert_state() {
    assert!(!should_check_daily_triage(
        TriageAlertEvent::PaletteEnabled,
        true,
        false,
    ));
    assert!(should_check_daily_triage(
        TriageAlertEvent::RefreshSucceeded,
        false,
        false,
    ));
    assert!(!should_check_daily_triage(
        TriageAlertEvent::RefreshSucceeded,
        false,
        true,
    ));
}

#[test]
fn resolves_when_a_newer_journal_row_appears() {
    // Same id → sync hasn't finished yet.
    assert!(!triage_gate_resolved(Some(5), Some(5)));
    // A newer row → a sync completed.
    assert!(triage_gate_resolved(Some(5), Some(6)));
}

#[test]
fn first_ever_row_resolves_from_an_empty_journal() {
    assert!(triage_gate_resolved(None, Some(1)));
    assert!(!triage_gate_resolved(None, None));
}

#[test]
fn does_not_resolve_at_the_deadline_without_a_completed_sync() {
    // A slow or offline sync must not make the gate evaluate stale local
    // habits. It remains closed until a newer journal row proves that a
    // sync completed.
    assert!(!triage_gate_resolved(Some(5), Some(5)));
    assert!(!triage_gate_resolved(None, None));
}

#[test]
fn successful_startup_sync_reloads_opposite_portable_config_and_task_state() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("family");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".config/config.json"),
        b"{\"enable_triage_habits\":false}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tasks/tasks.csv"),
        b"task_uuid,task_id,task_name,status,assigned_to,system_key\n",
    )
    .unwrap();
    std::fs::write(
            root.join("tasks/habits.csv"),
            b"task_uuid,task_id,task_name,status,assigned_to,system_key\n8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,H1,Morning Triage,not_started,member,brain.triage.daily\n",
        ).unwrap();
    let workspace = crate::workspace::WorkspaceContext::new(
        temporary.path(),
        crate::workspace::WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
        crate::workspace::WorkspaceName::parse("family").unwrap(),
        &root,
        "member",
        temporary.path(),
    )
    .unwrap();

    let refreshed = refresh_after_successful_startup_sync(&workspace).unwrap();

    assert!(!refreshed.config.enable_triage_habits);
    assert!(
        refreshed
            .habits
            .iter()
            .all(|habit| !habit.is_managed_triage())
    );
    assert!(
        crate::tasks::task::load_habits(&root.join("tasks/habits.csv"))
            .unwrap()
            .iter()
            .all(|habit| !habit.is_managed_triage())
    );
}

#[test]
fn successful_startup_sync_reads_config_after_acquiring_the_task_store_owner() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("family");
    std::fs::create_dir_all(root.join(".config")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".config/config.json"),
        b"{\"enable_triage_habits\":true}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tasks/tasks.csv"),
        b"task_uuid,task_id,task_name,status,assigned_to,system_key\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tasks/habits.csv"),
        b"task_uuid,task_id,task_name,status,assigned_to,system_key\n",
    )
    .unwrap();
    let workspace = crate::workspace::WorkspaceContext::new(
        temporary.path(),
        crate::workspace::WorkspaceId::parse("a09c6257-6ccc-4a39-97a4-058c73a8c569").unwrap(),
        crate::workspace::WorkspaceName::parse("family").unwrap(),
        &root,
        "member",
        temporary.path(),
    )
    .unwrap();
    let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(&workspace).unwrap();
    let refresh_workspace = workspace;
    let refresh = std::thread::spawn(move || {
        refresh_after_successful_startup_sync(&refresh_workspace).unwrap()
    });

    std::thread::sleep(std::time::Duration::from_millis(100));
    std::fs::write(
        root.join(".config/config.json"),
        b"{\"enable_triage_habits\":false}\n",
    )
    .unwrap();
    drop(owner);

    let refreshed = refresh.join().unwrap();
    assert!(!refreshed.config.enable_triage_habits);
    assert!(
        refreshed
            .habits
            .iter()
            .all(|habit| !habit.is_managed_triage())
    );
}

/// `.claude/settings.json` syncs, and startup installs hooks *before* the
/// startup pull, so a machine on an older brain can hand this one its stale
/// hook commands and silently win. Reasserting after the pull is what stops a
/// fixed hook path from coming back broken.
#[test]
fn a_pull_that_reverts_this_machines_hook_commands_is_corrected() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let home = temporary.path().join("home");
    let root = temporary.path().join("brain");
    std::fs::create_dir_all(root.join(".claude")).expect("workspace root");
    std::fs::create_dir_all(&home).expect("home");
    // Exactly what an older machine publishes and a pull brings down.
    std::fs::write(
        root.join(".claude/settings.json"),
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"python3 .claude/brain-hooks/agent_turn_complete_hook.py"}]}]},"permissions":{"allow":["Read"]}}"#,
    )
    .expect("stale settings from the remote");

    let workspace = crate::workspace::WorkspaceContext::new(
        &home,
        crate::workspace::WorkspaceId::parse("11111111-1111-4111-8111-111111111111")
            .expect("workspace id"),
        crate::workspace::WorkspaceName::parse("brain").expect("workspace name"),
        &root,
        "pablo",
        &root,
    )
    .expect("workspace context");

    super::reinstall_lifecycle_after_pull(&workspace);

    let settings = std::fs::read_to_string(root.join(".claude/settings.json"))
        .expect("settings after the pull was corrected");
    assert!(
        settings.contains("CLAUDE_PROJECT_DIR"),
        "the pulled-in stale command was left in place: {settings}"
    );
    assert!(
        !settings.contains("python3 .claude/brain-hooks"),
        "the relative command survived: {settings}"
    );
    assert!(
        settings.contains("Read"),
        "unrelated settings from the remote must survive: {settings}"
    );
}
