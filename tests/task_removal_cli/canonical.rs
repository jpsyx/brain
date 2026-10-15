use super::*;
#[path = "../support/task_removal_canonical.rs"]
mod fixture;

#[test]
fn canonical_middle_removal_cli_rewires_archived_project_and_agenda() {
    let f = Fixture::new();
    let date = chrono::Local::now().date_naive().to_string();
    let agenda = f.temp.path().join("agendas").join(format!("{date}.md"));
    fixture::seed(&f.root, &agenda);
    let before = fixture::images(&f.root, &agenda);
    let preview = document(
        &f.run(&[
            "tasks",
            "remove",
            "T2",
            "--dependents",
            "rewire",
            "--dry-run",
            "--json",
        ]),
        0,
    );
    assert_eq!(preview["ready"], true);
    assert_eq!(fixture::images(&f.root, &agenda), before);
    let receipt = document(
        &f.run(&[
            "tasks",
            "remove",
            "T2",
            "--dependents",
            "rewire",
            "--expect-plan",
            preview["fingerprint"].as_str().unwrap(),
            "--yes",
            "--json",
        ]),
        0,
    );
    assert_eq!(receipt["committed"], true);
    assert_eq!(receipt["removed"][0]["task_id"], "T2");
    fixture::assert_after(&f.root, &agenda, &date);
}
