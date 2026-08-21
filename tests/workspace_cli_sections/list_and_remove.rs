
#[test]
fn workspace_remove_detaches_an_alias_selected_record_and_leaves_root_contents() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    let work = fixture.home.path().join("work");
    for root in [&family, &work] {
        assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(root)]));
    }
    fixture.make_ready("family");
    let sentinel = work.join("keep.txt");
    std::fs::write(&sentinel, "never delete me").expect("sentinel");
    assert_success(&fixture.run(&["workspace", "alias", "add", "work", "job"]));

    let output = fixture.run(&["workspace", "remove", "job"]);

    assert_success(&output);
    let registry = fixture.registry();
    assert_eq!(registry.workspaces.len(), 1);
    assert!(!registry.workspaces.contains_key(&name("work")));
    assert_eq!(
        std::fs::read_to_string(sentinel).unwrap(),
        "never delete me"
    );
}

#[test]
fn workspace_list_is_sorted_complete_plain_and_accepts_a_global_alias_selector() {
    let fixture = Fixture::new();
    let zeta = fixture.home.path().join("zeta");
    let alpha = fixture.home.path().join("alpha");
    for root in [&zeta, &alpha] {
        assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(root)]));
    }
    fixture.make_ready("alpha");
    fixture.make_ready("zeta");
    assert_success(&fixture.run(&["workspace", "alias", "add", "alpha", "shared"]));
    assert_success(&fixture.run(&["workspace", "alias", "add", "alpha", "a"]));
    let mut registry = fixture.registry();
    let zeta_record = registry.workspaces.get_mut(&name("zeta")).unwrap();
    zeta_record.local_user_id = "user-z".to_owned();
    zeta_record.receiver_enabled = true;
    RegistryStore::from_path(fixture.registry_path())
        .replace(&registry)
        .unwrap();
    std::fs::create_dir_all(alpha.join(".config")).unwrap();
    std::fs::write(
        alpha.join(".config/config.json"),
        r#"{"access_mode":"workspace_only"}"#,
    )
    .unwrap();

    let output = fixture.run(&["-b", "A", "workspace", "list"]);

    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 list output");
    assert!(!stdout.contains('\x1b'));
    let workspace_rows = format!(
        "Workspaces\n\n  alpha\n    root: {}\n    aliases: a, shared\n    local user: test-user\n    receiver: disabled\n    Access mode  workspace-only\n    Enforcement  advisory prompts and capability filtering\n    Sandbox      none\n* zeta (default)\n    root: {}\n    aliases: none\n    local user: user-z\n    receiver: enabled\n    Access mode  unrestricted\n    Enforcement  frontend defaults\n    Sandbox      none\n",
        alpha.display(),
        zeta.display()
    );
    assert!(stdout.starts_with(&workspace_rows), "{stdout}");
    assert!(stdout.contains("Workspace alpha"), "{stdout}");
    assert!(stdout.contains("portable users: unavailable"), "{stdout}");
    assert!(
        stdout.contains("access policy (advisory; no isolation): ready"),
        "{stdout}"
    );
}

#[cfg(unix)]
#[test]
fn trailing_workspace_selector_forms_do_not_leak_into_binary_task_arguments() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    fixture.make_ready("family");
    let markdown_to_pdf = fixture.current_dir.path().join("markdown-to-pdf");
    fake_markdown_to_pdf(&markdown_to_pdf);
    let mut registry = fixture.registry();
    registry
        .workspaces
        .get_mut(&name("family"))
        .unwrap()
        .env
        .insert(
            "markdown_to_pdf_path".to_owned(),
            serde_json::json!(markdown_to_pdf),
        );
    RegistryStore::from_path(fixture.registry_path())
        .replace(&registry)
        .unwrap();
    let tasks_dir = family.join("tasks");
    std::fs::create_dir_all(&tasks_dir).expect("tasks directory");
    std::fs::write(
        tasks_dir.join("tasks.csv"),
        "task_id,task_name,task_type,status,priority,due_date,hard_deadline,start_date,assignee,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue\n",
    )
    .expect("empty tasks CSV");

    for args in [
        vec!["tasks", "today", "--brain", "family", "--no-tui"],
        vec!["tasks", "today", "-b", "family", "--no-tui"],
        vec!["tasks", "today", "--brain=family", "--no-tui"],
    ] {
        let output = fixture.run(&args);
        assert_success(&output);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("== Today =="),
            "unexpected stdout for {args:?}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn unknown_global_selector_reports_how_to_discover_valid_selectors() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));

    let output = fixture.run(&["--brain", "missing", "workspace", "list"]);

    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "unknown workspace selector missing",
            "brain workspace list",
        ],
    );
}

#[test]
fn a_bare_workspace_list_reports_health_for_every_registered_workspace() {
    let fixture = Fixture::new();
    for name in ["alpha", "zeta"] {
        let root = fixture.home.path().join(name);
        assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&root)]));
        fixture.make_ready(name);
    }

    // No `-w`: the question is "what does this machine have", so every
    // workspace's feature health is part of the answer.
    let output = fixture.run(&["workspace", "list"]);

    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 list output");
    assert!(stdout.contains("Workspace alpha"), "{stdout}");
    assert!(stdout.contains("Workspace zeta"), "{stdout}");
    assert_eq!(stdout.matches("  Features").count(), 2, "{stdout}");
}

#[test]
fn a_selected_workspace_list_reports_only_that_workspaces_health() {
    let fixture = Fixture::new();
    for name in ["alpha", "zeta"] {
        let root = fixture.home.path().join(name);
        assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&root)]));
        fixture.make_ready(name);
    }

    let output = fixture.run(&["workspace", "list", "-w", "alpha"]);

    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 list output");
    // Both are still inventoried in the header rows…
    assert!(stdout.contains("alpha (default)"), "{stdout}");
    assert!(stdout.contains("  zeta\n"), "{stdout}");
    // …but `-w` asked about one, so only its health is reported.
    assert!(stdout.contains("Workspace alpha"), "{stdout}");
    assert!(!stdout.contains("Workspace zeta"), "{stdout}");
    assert_eq!(stdout.matches("  Features").count(), 1, "{stdout}");
}

#[test]
fn a_workspace_that_still_needs_setup_never_takes_the_whole_inventory_down() {
    let fixture = Fixture::new();
    let ready = fixture.home.path().join("alpha");
    let unready = fixture.home.path().join("zeta");
    for root in [&ready, &unready] {
        assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(root)]));
    }
    fixture.make_ready("alpha");

    // `zeta` has no manifest or portable user yet; listing must still work.
    let output = fixture.run(&["workspace", "list"]);

    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 list output");
    assert!(stdout.contains("Workspace alpha"), "{stdout}");
    assert!(stdout.contains("Workspace zeta"), "{stdout}");
}
