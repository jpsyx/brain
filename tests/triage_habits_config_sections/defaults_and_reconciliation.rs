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
