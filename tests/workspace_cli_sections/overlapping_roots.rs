
#[test]
fn overlapping_workspace_root_reports_the_safe_root_remedy_without_creating_it() {
    let fixture = Fixture::new();
    let family = fixture.home.path().join("family");
    assert_success(&fixture.run(&["workspace", "create", "--root", path_arg(&family)]));
    let nested = family.join("nested");

    let output = fixture.run(&["workspace", "create", "--root", path_arg(&nested)]);

    assert_failure_contains(
        &output,
        &[
            "Workspace error:",
            "overlap",
            "outside every registered workspace",
        ],
    );
    assert!(!nested.exists());
}
