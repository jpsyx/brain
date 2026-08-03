use brain::config::Config;
use brain::tasks::task::load_habits;
use brain::tasks::triage_habits::{
    DAILY_SYSTEM_KEY, ManagedTaskError, WEEKLY_SYSTEM_KEY, apply_triage_habits_config,
    can_complete, can_remove, can_revive, can_skip,
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

#[test]
fn managed_triage_habits_are_enabled_by_default() {
    assert!(Config::default().enable_triage_habits);
}

#[test]
fn config_schema_exposes_the_enabled_default() {
    let (_temporary, workspace) = empty_workspace();

    assert_eq!(
        brain::settings::resolve_one(&workspace, "enable_triage_habits").as_deref(),
        Some("true")
    );
}

#[test]
fn enabling_reconciles_one_open_daily_and_weekly_chain() {
    let (_temporary, workspace) = empty_workspace();

    apply_triage_habits_config(&workspace, true).unwrap();
    apply_triage_habits_config(&workspace, true).unwrap();

    let habits = load_habits(&workspace.root().join("tasks/habits.csv")).unwrap();
    for key in [DAILY_SYSTEM_KEY, WEEKLY_SYSTEM_KEY] {
        let chain = habits
            .iter()
            .filter(|habit| habit.system_key == key)
            .collect::<Vec<_>>();
        assert_eq!(chain.len(), 1, "{key} should have one current row");
        assert!(!chain[0].is_done());
        assert!(chain[0].task_uuid.is_some());
        assert_eq!(chain[0].assigned_to, "member");
    }
}

#[test]
fn enabling_removes_duplicate_open_legacy_rows_without_uuids() {
    let (_temporary, workspace) = empty_workspace();
    apply_triage_habits_config(&workspace, true).unwrap();
    let habits_path = workspace.root().join("tasks/habits.csv");
    let mut habits = std::fs::read_to_string(&habits_path).unwrap();
    habits.push_str(
        ",H998,Legacy ordinary habit,not_started,p1,2026-08-03,false,member,,,,,,,,1,days,2026-08-03,,,,\n",
    );
    habits.push_str(
        ",H999,Legacy duplicate,not_started,p1,2026-08-03,false,member,,,,,,,,1,days,2026-08-03,,,brain.triage.daily\n",
    );
    std::fs::write(&habits_path, habits).unwrap();

    apply_triage_habits_config(&workspace, true).unwrap();

    let habits = load_habits(&habits_path).unwrap();
    assert_eq!(
        habits
            .iter()
            .filter(|habit| habit.system_key == DAILY_SYSTEM_KEY && !habit.is_done())
            .count(),
        1
    );
    assert!(habits.iter().any(|habit| habit.id == "H998"));
}

#[test]
fn system_marker_protects_managed_rows_independently_of_the_visible_name() {
    let (_temporary, workspace) = empty_workspace();
    apply_triage_habits_config(&workspace, true).unwrap();
    let habits_path = workspace.root().join("tasks/habits.csv");
    let mut body = std::fs::read_to_string(&habits_path).unwrap();
    body = body.replace("Morning Triage", "Renamed by a user");
    std::fs::write(&habits_path, body).unwrap();

    apply_triage_habits_config(&workspace, true).unwrap();

    let habits = load_habits(&habits_path).unwrap();
    let managed = habits
        .iter()
        .find(|habit| habit.system_key == DAILY_SYSTEM_KEY)
        .unwrap();
    assert_eq!(managed.name, "Renamed by a user");
    assert!(managed.is_managed_triage());
    let enabled = Config::default();
    assert!(matches!(
        can_remove(managed, &enabled),
        Err(ManagedTaskError::ManagedTaskCannotDelete)
    ));
    assert!(matches!(
        can_complete(managed, &enabled),
        Err(ManagedTaskError::ManagedTaskCannotComplete)
    ));
    assert!(matches!(
        can_revive(managed, &enabled),
        Err(ManagedTaskError::ManagedTaskCannotRevive)
    ));
    assert!(matches!(
        can_skip(managed, &enabled),
        Err(ManagedTaskError::ManagedTaskCannotSkip)
    ));

    let disabled: Config = serde_json::from_str(r#"{"enable_triage_habits":false}"#).unwrap();
    assert!(can_remove(managed, &disabled).is_ok());
}

fn csv_rows(path: &std::path::Path) -> Vec<std::collections::BTreeMap<String, String>> {
    csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .unwrap()
        .deserialize()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn disabling_purges_every_managed_row_and_derived_reference_then_reenables_fresh() {
    let (_temporary, workspace) = empty_workspace();
    apply_triage_habits_config(&workspace, true).unwrap();
    let root = workspace.root();
    let tasks_path = root.join("tasks/tasks.csv");
    let habits_path = root.join("tasks/habits.csv");
    let original = load_habits(&habits_path).unwrap();
    let daily = original
        .iter()
        .find(|habit| habit.system_key == DAILY_SYSTEM_KEY)
        .unwrap();
    let old_daily_uuid = daily.task_uuid.unwrap().to_string();
    let old_daily_id = daily.id.clone();

    brain::tasks::complete::complete_in_root_with_today(
        root,
        &old_daily_id,
        chrono::Local::now().date_naive(),
    )
    .unwrap();
    let after_completion = load_habits(&habits_path).unwrap();
    let next_daily = after_completion
        .iter()
        .find(|habit| {
            habit.system_key == DAILY_SYSTEM_KEY && !habit.is_done() && habit.id != old_daily_id
        })
        .unwrap();
    assert!(next_daily.task_uuid.is_some());
    assert_eq!(next_daily.assigned_to, "member");
    let mut habits = std::fs::read_to_string(&habits_path).unwrap();
    habits.push_str(
        "7d49b547-1d9f-439b-bd97-b98327ecae20,H900,Morning Triage,not_started,p2,2026-08-03,false,member,,,,,,,,1,days,2026-08-03,,,\n",
    );
    std::fs::write(&habits_path, habits).unwrap();
    std::fs::write(
        &tasks_path,
        format!(
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n\
             5b68bb73-8ac4-42c4-8a55-7ab8f9112ca7,T44,Generated triage follow-up,not_started,member,{DAILY_SYSTEM_KEY}\n\
             d2ec63ee-a425-4875-a79d-1ec6a8165414,T45,Keep ordinary task,not_started,member,\n"
        ),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("projects/example")).unwrap();
    std::fs::write(
        root.join("projects/example/.METADATA.json"),
        format!("{{\"tasks\":[\"{old_daily_uuid}\",\"d2ec63ee-a425-4875-a79d-1ec6a8165414\"]}}\n"),
    )
    .unwrap();
    let derived = root.join("tasks/agenda-index.md");
    std::fs::write(
        &derived,
        format!(
            "managed {old_daily_uuid}\nmanaged-id {old_daily_id}\nordinary-display H1000\nordinary d2ec63ee-a425-4875-a79d-1ec6a8165414\n"
        ),
    )
    .unwrap();
    let transcript = root.join("resources/unrelated-transcript.md");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(
        &transcript,
        format!("discussion mentions {old_daily_uuid}\n"),
    )
    .unwrap();

    apply_triage_habits_config(&workspace, false).unwrap();

    let all_rows = csv_rows(&tasks_path)
        .into_iter()
        .chain(csv_rows(&habits_path))
        .collect::<Vec<_>>();
    assert!(all_rows.iter().all(|row| {
        !matches!(
            row.get("system_key").map(String::as_str),
            Some(DAILY_SYSTEM_KEY | WEEKLY_SYSTEM_KEY)
        )
    }));
    assert!(all_rows.iter().any(
        |row| row.get("task_uuid") == Some(&"7d49b547-1d9f-439b-bd97-b98327ecae20".to_owned())
    ));
    assert!(
        !std::fs::read_to_string(&derived)
            .unwrap()
            .contains(&old_daily_uuid)
    );
    assert!(
        std::fs::read_to_string(&derived)
            .unwrap()
            .contains("d2ec63ee-a425-4875-a79d-1ec6a8165414")
    );
    assert!(
        std::fs::read_to_string(&derived)
            .unwrap()
            .contains("ordinary-display H1000")
    );
    assert!(transcript.exists());
    assert!(
        std::fs::read_to_string(&transcript)
            .unwrap()
            .contains(&old_daily_uuid)
    );
    assert!(!Config::load(&workspace).enable_triage_habits);

    apply_triage_habits_config(&workspace, true).unwrap();

    let recreated = load_habits(&habits_path).unwrap();
    assert_eq!(
        recreated
            .iter()
            .filter(|row| row.is_managed_triage())
            .count(),
        2
    );
    assert!(
        recreated
            .iter()
            .filter(|row| row.is_managed_triage())
            .all(|row| !row.is_done())
    );
    assert!(
        recreated
            .iter()
            .filter_map(|row| row.task_uuid)
            .all(|uuid| uuid.to_string() != old_daily_uuid)
    );
}

#[cfg(unix)]
#[test]
fn failed_config_write_rolls_csvs_back_to_the_prior_generation() {
    use std::os::unix::fs::PermissionsExt;

    let (_temporary, workspace) = empty_workspace();
    apply_triage_habits_config(&workspace, true).unwrap();
    let root = workspace.root();
    let tasks_path = root.join("tasks/tasks.csv");
    let habits_path = root.join("tasks/habits.csv");
    let config_path = root.join(".config/config.json");
    let before_tasks = std::fs::read(&tasks_path).unwrap();
    let before_habits = std::fs::read(&habits_path).unwrap();
    std::fs::remove_file(&config_path).unwrap();
    let config_dir = config_path.parent().unwrap();
    std::fs::set_permissions(config_dir, std::fs::Permissions::from_mode(0o500)).unwrap();

    let result = apply_triage_habits_config(&workspace, false);

    std::fs::set_permissions(config_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(result.is_err());
    assert_eq!(std::fs::read(&tasks_path).unwrap(), before_tasks);
    assert_eq!(std::fs::read(&habits_path).unwrap(), before_habits);
    assert!(!config_path.exists());
}

#[test]
fn public_habit_mutators_reject_managed_triage_rows_while_enabled() {
    let (_temporary, workspace) = empty_workspace();
    apply_triage_habits_config(&workspace, true).unwrap();
    let habits_path = workspace.root().join("tasks/habits.csv");
    let before = std::fs::read(&habits_path).unwrap();
    let daily = load_habits(&habits_path)
        .unwrap()
        .into_iter()
        .find(|habit| habit.system_key == DAILY_SYSTEM_KEY)
        .unwrap();
    let actor = actor(&workspace);

    let complete = brain::tasks::complete::run(&workspace, &daily.id, &actor).unwrap_err();
    assert!(format!("{complete:#}").contains("cannot be completed outside triage"));
    assert_eq!(std::fs::read(&habits_path).unwrap(), before);

    let skip = brain::tasks::skip::run(&workspace, &daily.id, None, &actor).unwrap_err();
    assert!(format!("{skip:#}").contains("cannot be skipped manually"));
    assert_eq!(std::fs::read(&habits_path).unwrap(), before);

    let mut lapsed = String::from_utf8(before).unwrap();
    lapsed = lapsed.replace(",not_started,", ",done,");
    let first_row = lapsed.find('\n').unwrap() + 1;
    lapsed.insert_str(
        first_row,
        "7d49b547-1d9f-439b-bd97-b98327ecae20,H999,Morning Triage,done,p1,2026-08-03,false,member,,,,,,,,1,days,2026-08-03,2026-08-03,2026-08-03,\n",
    );
    std::fs::write(&habits_path, lapsed).unwrap();
    let before_revive = std::fs::read(&habits_path).unwrap();
    let revive = brain::tasks::revive::run(&workspace, "Morning Triage", &actor).unwrap_err();
    assert!(format!("{revive:#}").contains("cannot be revived manually"));
    assert_eq!(std::fs::read(&habits_path).unwrap(), before_revive);
}

#[test]
fn portable_config_set_runs_reconciliation_and_rejects_non_booleans() {
    let (_temporary, workspace) = empty_workspace();
    apply_triage_habits_config(&workspace, true).unwrap();
    assert_eq!(
        load_habits(&workspace.root().join("tasks/habits.csv"))
            .unwrap()
            .iter()
            .filter(|habit| habit.is_managed_triage())
            .count(),
        2
    );

    brain::settings::set(&workspace, "enable_triage_habits", "false").unwrap();

    assert!(!Config::load(&workspace).enable_triage_habits);
    assert!(
        load_habits(&workspace.root().join("tasks/habits.csv"))
            .unwrap()
            .iter()
            .all(|habit| !habit.is_managed_triage())
    );
    let before = std::fs::read(workspace.root().join(".config/config.json")).unwrap();
    let error = brain::settings::set(&workspace, "enable_triage_habits", "sometimes").unwrap_err();
    assert!(format!("{error:#}").contains("true or false"));
    assert_eq!(
        std::fs::read(workspace.root().join(".config/config.json")).unwrap(),
        before
    );
}

#[test]
fn bundled_skills_gate_only_managed_habit_mutation_when_feature_is_disabled() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let triage = std::fs::read_to_string(root.join("skills/triage/SKILL.md")).unwrap();
    let todo = std::fs::read_to_string(root.join("skills/todo/SKILL.md")).unwrap();

    for contract in [
        "brain config get enable_triage_habits",
        "--complete-managed-triage daily",
        "--complete-managed-triage weekly",
        "still run the full manual triage workflow",
        "send the background completion signal whether managed habits are enabled or disabled",
    ] {
        assert!(
            triage.contains(contract),
            "triage skill is missing {contract:?}"
        );
    }
    assert!(todo.contains("system_key=brain.triage.daily"));
    assert!(todo.contains("system_key=brain.triage.weekly"));
    assert!(todo.contains("managed triage rows cannot be removed"));
}
