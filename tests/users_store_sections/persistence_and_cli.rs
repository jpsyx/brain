
#[test]
fn canonical_users_round_trip_is_byte_stable() {
    let users = Users::parse(FIXTURE.as_bytes()).unwrap();
    let canonical = users.to_bytes().unwrap();
    let reparsed = Users::parse(&canonical).unwrap();

    assert_eq!(reparsed.to_bytes().unwrap(), canonical);
    assert!(canonical.ends_with(b"\n"));
}

#[test]
fn workspace_store_loads_and_atomically_saves_canonical_users() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = workspace(fixture.path());
    let users = Users::parse(FIXTURE.as_bytes()).unwrap();

    UsersStore::save(&workspace, &users).unwrap();
    let stored = std::fs::read(UsersStore::path(&workspace)).unwrap();
    assert_eq!(stored, users.to_bytes().unwrap());
    assert_eq!(UsersStore::load(&workspace).unwrap(), users);
    assert_eq!(
        UsersStore::path(&workspace),
        fixture.path().join(".config/users.json")
    );
    assert_eq!(
        std::fs::read_dir(fixture.path().join(".config"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn selected_workspace_user_cli_adds_updates_lists_and_selects_the_local_person() {
    let fixture = CliFixture::new();

    let add = fixture.run(&[
        "-b",
        "family",
        "user",
        "add",
        "--id",
        "alex-smith",
        "--name",
        "Alex Smith",
        "--phone",
        "646-555-0100",
        "--email",
        "Alex@Example.COM",
        "--response-email",
        "alex@example.com",
    ]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );

    let update = fixture.run(&[
        "user",
        "update",
        "alex-smith",
        "--name",
        "Alex Rivera",
        "--add-phone",
        "+16465550101",
        "--add-email",
        "alex+brain@example.com",
        "-b",
        "family",
    ]);
    assert!(
        update.status.success(),
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );

    let list = fixture.run(&["user", "list", "-b", "family"]);
    assert!(list.status.success());
    let stdout = String::from_utf8(list.stdout).unwrap();
    assert!(stdout.contains("alex-smith"));
    assert!(stdout.contains("Alex Rivera"));
    assert!(stdout.contains("+16465550100"));
    assert!(stdout.contains("alex@example.com"));

    let before_users = std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap();
    let local = fixture.run(&["-b", "family", "user", "local", "alex-smith"]);
    assert!(
        local.status.success(),
        "{}",
        String::from_utf8_lossy(&local.stderr)
    );
    assert_eq!(
        std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap(),
        before_users
    );
    let registry = RegistryStore::load_from(&fixture.registry_path).unwrap();
    assert_eq!(
        registry
            .select(Some("family"))
            .unwrap()
            .record()
            .local_user_id,
        "alex-smith"
    );
}

#[test]
fn removing_an_assigned_user_refuses_or_reassigns_tasks_without_partial_changes() {
    let fixture = CliFixture::new();
    let tasks_path = fixture.root.join("tasks/tasks.csv");
    std::fs::write(
        &tasks_path,
        "task_id,task_name,assignee,status\nT001,Plan trip,pablo,not_started\n",
    )
    .unwrap();
    let users_before = std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap();
    let tasks_before = std::fs::read(&tasks_path).unwrap();

    let refused = fixture.run(&["user", "remove", "pablo", "-b", "family"]);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("tasks remain assigned to pablo; use --reassign-to <USER_ID>")
    );
    assert_eq!(
        std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap(),
        users_before
    );
    assert_eq!(std::fs::read(&tasks_path).unwrap(), tasks_before);

    let invalid = fixture.run(&[
        "user",
        "remove",
        "pablo",
        "--reassign-to",
        "missing-user",
        "-b",
        "family",
    ]);
    assert!(!invalid.status.success());
    assert_eq!(
        std::fs::read(UsersStore::path(&workspace(&fixture.root))).unwrap(),
        users_before
    );
    assert_eq!(std::fs::read(&tasks_path).unwrap(), tasks_before);

    let removed = fixture.run(&[
        "-b",
        "family",
        "user",
        "remove",
        "pablo",
        "--reassign-to",
        "wife",
    ]);
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(
        fixture
            .users()
            .user(&UserId::parse("pablo").unwrap())
            .is_none()
    );
    assert!(
        String::from_utf8(std::fs::read(&tasks_path).unwrap())
            .unwrap()
            .starts_with("task_id,task_name,assigned_to,status\nT001,Plan trip,wife,not_started")
    );
}
