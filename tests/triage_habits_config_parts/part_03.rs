
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
fn malformed_or_non_object_config_aborts_before_mutating_managed_data() {
    for invalid in [b"not json\n".as_slice(), b"[]\n".as_slice()] {
        let (_temporary, workspace) = empty_workspace();
        let config_path = workspace.root().join(".config/config.json");
        let habits_path = workspace.root().join("tasks/habits.csv");
        std::fs::write(&config_path, invalid).unwrap();
        let before_config = std::fs::read(&config_path).unwrap();
        let before_habits = std::fs::read(&habits_path).unwrap();

        let error = apply_triage_habits_config(&workspace, true).unwrap_err();

        assert!(format!("{error:#}").contains("config.json"));
        assert_eq!(std::fs::read(&config_path).unwrap(), before_config);
        assert_eq!(std::fs::read(&habits_path).unwrap(), before_habits);
    }
}

#[test]
fn bundled_skills_gate_only_managed_habit_mutation_when_feature_is_disabled() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let triage = std::fs::read_to_string(root.join("skills/triage/SKILL.md")).unwrap();
    let todo = std::fs::read_to_string(root.join("skills/todo/SKILL.md")).unwrap();

    for contract in [
        "brain --brain \"$BRAIN_WORKSPACE\" config get enable_triage_habits",
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
