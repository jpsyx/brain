use brain::config::Config;
use brain::tasks::task::load_habits;
use brain::tasks::triage_habits::{
    DAILY_SYSTEM_KEY, ManagedTaskError, WEEKLY_SYSTEM_KEY, apply_triage_habits_config, can_remove,
    can_revive, can_skip,
};

fn workspace(root: &std::path::Path) -> brain::workspace::WorkspaceContext {
    brain::workspace::WorkspaceContext::new(
        root,
        brain::workspace::WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
        brain::workspace::WorkspaceName::parse("family").unwrap(),
        root,
        "member",
        root,
    )
    .unwrap()
}

fn empty_workspace() -> (tempfile::TempDir, brain::workspace::WorkspaceContext) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    std::fs::create_dir_all(root.join(".config")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(root.join(".config/config.json"), b"{}\n").unwrap();
    std::fs::write(
        root.join("tasks/tasks.csv"),
        b"task_uuid,task_id,task_name,status,assigned_to,system_key\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tasks/habits.csv"),
        b"task_uuid,task_id,task_name,status,priority,due_date,hard_deadline,assigned_to,see_also,notes,project,energy_level,context,estimated_duration,ideal_time,recur_interval,recur_unit,created_date,completed_date,last_touched,system_key\n",
    )
    .unwrap();
    let context = workspace(root);
    (temporary, context)
}

fn actor(workspace: &brain::workspace::WorkspaceContext) -> brain::actor::ActorContext {
    brain::actor::local_actor(workspace).unwrap()
}
