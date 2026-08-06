
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

    let habits_before = std::fs::read_to_string(&habits_path).unwrap();
    let habits_done = habits_before.replacen(",not_started,", ",done,", 1);
    std::fs::write(&habits_path, habits_done).unwrap();
    apply_triage_habits_config(&workspace, true).unwrap();
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
