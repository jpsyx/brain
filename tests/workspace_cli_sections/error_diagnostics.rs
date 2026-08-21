
#[test]
fn workspace_command_error_prints_its_display_once_without_debug_causes() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));

    let output = fixture.run(&["--brain", "missing", "workspace", "list"]);

    assert!(!output.status.success(), "command unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 workspace error");
    let failure = "unknown workspace selector missing";
    let message = "Workspace error: unknown workspace selector missing; run `brain workspace list` to see available names and aliases\n";
    assert_eq!(stderr, message);
    assert_eq!(stderr.matches(message.trim_end()).count(), 1);
    assert_eq!(stderr.matches(failure).count(), 1);
    assert!(
        !stderr.contains("Caused by:"),
        "unexpected source dump: {stderr:?}"
    );
}

#[test]
fn duplicate_workspace_name_reports_the_unique_name_remedy() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    let other = fixture.home.path().join("other");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));

    let output = fixture.run(&[
        "workspace",
        "create",
        "--name",
        "family",
        "--root",
        path_arg(&other),
    ]);

    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "workspace family already exists",
            "unique canonical name",
        ],
    );
}

#[test]
fn duplicate_workspace_alias_reports_the_unique_selector_remedy() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    let work = fixture.home.path().join("work");
    for root in [&family, &work] {
        assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(root)]));
    }
    fixture.make_ready("family");

    let output = fixture.run(&["workspace", "alias", "add", "work", "family"]);

    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "workspace selector family is not unique",
            "unique canonical name or alias",
        ],
    );
}

#[test]
fn duplicate_alias_on_the_same_workspace_fails_without_changing_registry_bytes() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    fixture.make_ready("family");
    assert_success(&fixture.run(&["workspace", "alias", "add", "family", "alt"]));
    let registry_bytes = std::fs::read(fixture.registry_path()).expect("registry bytes");

    let output = fixture.run(&["workspace", "alias", "add", "family", "ALT"]);

    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "workspace family already has alias alt",
            "remove the existing alias or choose a different one",
        ],
    );
    assert_eq!(
        std::fs::read(fixture.registry_path()).expect("registry bytes after failure"),
        registry_bytes
    );
}
