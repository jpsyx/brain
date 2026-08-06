
#[test]
fn reassigning_a_legacy_assignment_value_moves_work_without_inventing_a_person() {
    let fixture = CliFixture::new();
    let tasks_path = fixture.root.join("tasks/tasks.csv");
    let habits_path = fixture.root.join("tasks/habits.csv");
    std::fs::write(
        &tasks_path,
        "task_id,task_name,assigned_to,status\nT001,Plan trip,me,not_started\nT002,Rest,wife,not_started\n",
    )
    .unwrap();
    std::fs::write(&habits_path, "task_id,task_name,assigned_to\nH1,Walk,me\n").unwrap();
    let users_before = std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap();
    let tasks_before = std::fs::read(&tasks_path).unwrap();

    let unknown = fixture.run(&["user", "reassign", "me", "nobody", "-b", "family"]);
    assert!(!unknown.status.success());
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("nobody"),
        "{}",
        String::from_utf8_lossy(&unknown.stderr)
    );
    assert_eq!(std::fs::read(&tasks_path).unwrap(), tasks_before);

    let reassigned = fixture.run(&["user", "reassign", "me", "pablo", "-b", "family"]);

    assert!(
        reassigned.status.success(),
        "{}",
        String::from_utf8_lossy(&reassigned.stderr)
    );
    assert_eq!(
        String::from_utf8(std::fs::read(&tasks_path).unwrap()).unwrap(),
        "task_id,task_name,assigned_to,status\nT001,Plan trip,pablo,not_started\nT002,Rest,wife,not_started\n"
    );
    assert_eq!(
        String::from_utf8(std::fs::read(&habits_path).unwrap()).unwrap(),
        "task_id,task_name,assigned_to\nH1,Walk,pablo\n"
    );
    assert_eq!(
        std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap(),
        users_before,
        "reassignment never adds or removes a portable person"
    );
}

#[test]
fn reassigning_an_absent_value_reports_it_and_leaves_every_task_byte_alone() {
    let fixture = CliFixture::new();
    let tasks_path = fixture.root.join("tasks/tasks.csv");
    std::fs::write(
        &tasks_path,
        "task_id,task_name,assigned_to\nT001,Plan trip,pablo\n",
    )
    .unwrap();
    std::fs::write(
        fixture.root.join("tasks/habits.csv"),
        "task_id,task_name,assigned_to\nH1,Walk,pablo\n",
    )
    .unwrap();
    let before = std::fs::read(&tasks_path).unwrap();

    let output = fixture.run(&["user", "reassign", "ghost", "wife", "-b", "family"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ghost"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(std::fs::read(&tasks_path).unwrap(), before);
}

#[test]
fn user_removal_collapses_both_assignment_headers_and_prefers_canonical_values() {
    let fixture = CliFixture::new();
    let tasks_path = fixture.root.join("tasks/tasks.csv");
    std::fs::write(
        &tasks_path,
        "task_id,task_name,assignee,assigned_to,status\nT001,Plan trip,pablo,wife,not_started\n",
    )
    .unwrap();

    let removed = fixture.run(&["user", "remove", "pablo", "-b", "family"]);

    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert_eq!(
        String::from_utf8(std::fs::read(tasks_path).unwrap()).unwrap(),
        "task_id,task_name,assigned_to,status\nT001,Plan trip,wife,not_started\n"
    );
}

#[test]
fn ordinary_commands_reject_a_local_user_not_in_the_portable_registry() {
    let fixture = CliFixture::new();
    let mut registry = RegistryStore::load_from(&fixture.registry_path).unwrap();
    registry
        .workspaces
        .get_mut(&WorkspaceName::parse("family").unwrap())
        .unwrap()
        .local_user_id = "missing-user".to_owned();
    RegistryStore::from_path(fixture.registry_path.clone())
        .replace(&registry)
        .unwrap();

    let output = fixture.run(&["config", "list", "-b", "family"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("local user missing-user is not a portable member"));
    assert!(stderr.contains("brain user local <USER_ID> -b family"));
}
