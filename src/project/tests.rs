//! Scaffolding, editing, and archiving a project — and the one judgement-free
//! signal that says a project stopped rather than finished.

use chrono::NaiveDate;

use super::{archive, create, locate, model, set, show};

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 8, 24).expect("valid date")
}

fn brain() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("tasks")).expect("tasks dir");
    std::fs::write(
        dir.path().join("tasks/tasks.csv"),
        "task_id,task_name,status,project,last_touched,created_date,due_date\n",
    )
    .expect("tasks.csv");
    dir
}

fn seed(root: &std::path::Path) {
    create(
        root,
        "work__apply-to-conference",
        "Apply to the conference",
        "in-progress",
        "p1",
        "2026-09-15",
        "Submit the talk proposal.",
    )
    .expect("create");
}

#[test]
fn a_new_project_gets_the_full_metadata_record() {
    let dir = brain();
    seed(dir.path());

    let metadata =
        model::load(&dir.path().join("projects/work__apply-to-conference")).expect("metadata");

    assert_eq!(metadata.name, "work__apply-to-conference");
    assert_eq!(metadata.namespace, "work");
    assert_eq!(metadata.title, "Apply to the conference");
    assert_eq!(metadata.status, "in-progress");
    assert_eq!(metadata.priority, "p1");
    assert_eq!(metadata.due, "2026-09-15");
    assert_eq!(metadata.directory, "projects/work__apply-to-conference");
    assert!(metadata.tasks.is_empty());
}

#[test]
fn a_new_project_gets_a_readme_with_no_second_copy_of_the_metadata() {
    let dir = brain();
    seed(dir.path());

    let readme = std::fs::read_to_string(
        dir.path()
            .join("projects/work__apply-to-conference/README.md"),
    )
    .expect("readme");

    assert_eq!(
        readme,
        "# Apply to the conference\n\nSubmit the talk proposal.\n"
    );
    // Status and dates live in the metadata; a copy here would only rot.
    assert!(!readme.contains("status"), "{readme}");
    assert!(!readme.contains("2026-09-15"), "{readme}");
}

#[test]
fn creating_over_an_existing_project_is_refused() {
    let dir = brain();
    seed(dir.path());

    let error = create(
        dir.path(),
        "work__apply-to-conference",
        "Something else",
        "not-started",
        "p2",
        "none",
        "",
    )
    .expect_err("already exists");

    assert!(error.to_string().contains("already exists"), "{error}");
    // The original survived untouched.
    let metadata =
        model::load(&dir.path().join("projects/work__apply-to-conference")).expect("metadata");
    assert_eq!(metadata.title, "Apply to the conference");
}

#[test]
fn a_slug_without_a_namespace_is_refused() {
    let dir = brain();
    for slug in ["apply-to-conference", "__outcome", "work__", "Work__Thing"] {
        assert!(
            create(dir.path(), slug, "Title", "not-started", "p2", "none", "").is_err(),
            "{slug}"
        );
    }
}

#[test]
fn an_unsortable_due_date_is_refused() {
    let dir = brain();
    // A project's due date is exactly what gets sorted by.
    assert!(
        create(
            dir.path(),
            "work__x",
            "T",
            "not-started",
            "p2",
            "next month",
            ""
        )
        .is_err()
    );
    assert_eq!(model::validate_due("none").expect("none"), "none");
    assert_eq!(model::validate_due("").expect("empty"), "none");
    assert_eq!(
        model::validate_due("2026-01-05").expect("date"),
        "2026-01-05"
    );
}

#[test]
fn an_unknown_status_or_priority_is_refused() {
    let dir = brain();
    assert!(create(dir.path(), "work__x", "T", "finished", "p2", "none", "").is_err());
    assert!(
        create(
            dir.path(),
            "work__x",
            "T",
            "not-started",
            "urgent",
            "none",
            ""
        )
        .is_err()
    );
}

#[test]
fn setting_reports_only_what_actually_changed() {
    let dir = brain();
    seed(dir.path());

    let (_, changes) = set(
        dir.path(),
        "work__apply-to-conference",
        None,
        Some("done"),
        Some("p1"),
        None,
    )
    .expect("set");

    // p1 was already p1.
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].field, "status");
    assert_eq!(changes[0].before, "in-progress");
    assert_eq!(changes[0].after, "done");
}

#[test]
fn setting_nothing_is_refused_rather_than_rewriting_the_file() {
    let dir = brain();
    seed(dir.path());

    let error = set(
        dir.path(),
        "work__apply-to-conference",
        None,
        None,
        None,
        None,
    )
    .expect_err("no fields");

    assert!(error.to_string().contains("no fields given"), "{error}");
}

#[test]
fn unknown_metadata_fields_survive_a_write() {
    let dir = brain();
    seed(dir.path());
    let directory = dir.path().join("projects/work__apply-to-conference");
    let mut metadata = model::load(&directory).expect("metadata");
    metadata.extra.insert(
        "deleted_backlog_tasks".to_owned(),
        serde_json::json!([{ "task_id": "T9" }]),
    );
    model::save(&directory, &metadata).expect("save");

    set(
        dir.path(),
        "work__apply-to-conference",
        None,
        Some("done"),
        None,
        None,
    )
    .expect("set");

    let after = std::fs::read_to_string(model::metadata_path(&directory)).expect("read");
    assert!(after.contains("deleted_backlog_tasks"), "{after}");
    assert!(after.contains("\"T9\""), "{after}");
}

#[test]
fn archiving_preserves_the_folder_name_and_repoints_the_record() {
    let dir = brain();
    seed(dir.path());

    let located = archive(dir.path(), "work__apply-to-conference").expect("archive");

    assert!(located.archived);
    assert_eq!(
        located.relative,
        "archive/projects/work__apply-to-conference"
    );
    assert!(
        !dir.path()
            .join("projects/work__apply-to-conference")
            .exists()
    );
    let metadata = model::load(&located.directory).expect("metadata");
    assert_eq!(
        metadata.directory,
        "archive/projects/work__apply-to-conference"
    );
    assert_eq!(metadata.name, "work__apply-to-conference");
}

#[test]
fn an_archived_project_is_still_found_and_cannot_be_archived_twice() {
    let dir = brain();
    seed(dir.path());
    archive(dir.path(), "work__apply-to-conference").expect("archive");

    assert!(
        locate(dir.path(), "work__apply-to-conference")
            .expect("locate")
            .archived
    );
    let error = archive(dir.path(), "work__apply-to-conference").expect_err("twice");
    assert!(error.to_string().contains("already archived"), "{error}");
}

#[test]
fn an_unknown_project_says_where_it_looked() {
    let dir = brain();
    let error = locate(dir.path(), "work__nothing").expect_err("unknown");
    assert!(error.to_string().contains("archive/projects/"), "{error}");
}

fn with_tasks(root: &std::path::Path, rows: &str) {
    std::fs::write(
        root.join("tasks/tasks.csv"),
        format!("task_id,task_name,status,project,last_touched,created_date,due_date\n{rows}"),
    )
    .expect("tasks.csv");
}

#[test]
fn a_project_whose_every_open_task_is_ignored_died_quietly() {
    let dir = brain();
    seed(dir.path());
    with_tasks(
        dir.path(),
        "T1,Draft,not_started,work__apply-to-conference,2026-01-01,2026-01-01,\n\
         T2,Send,not_started,work__apply-to-conference,2026-02-01,2026-02-01,\n",
    );

    let report = show(dir.path(), "work__apply-to-conference", today()).expect("show");

    assert_eq!(report.open_tasks, ["T1", "T2"]);
    assert_eq!(report.ignored_tasks, ["T1", "T2"]);
    assert!(report.died_quietly);
}

#[test]
fn one_live_task_is_enough_for_a_project_to_be_alive() {
    let dir = brain();
    seed(dir.path());
    with_tasks(
        dir.path(),
        "T1,Draft,not_started,work__apply-to-conference,2026-01-01,2026-01-01,\n\
         T2,Send,not_started,work__apply-to-conference,2026-08-23,2026-08-01,\n",
    );

    let report = show(dir.path(), "work__apply-to-conference", today()).expect("show");

    assert_eq!(report.ignored_tasks, ["T1"]);
    assert!(!report.died_quietly);
}

#[test]
fn a_project_with_no_open_tasks_has_not_died_quietly() {
    let dir = brain();
    seed(dir.path());
    with_tasks(
        dir.path(),
        "T1,Draft,done,work__apply-to-conference,2026-01-01,2026-01-01,\n",
    );

    let report = show(dir.path(), "work__apply-to-conference", today()).expect("show");

    // Finished is not abandoned.
    assert!(report.open_tasks.is_empty());
    assert!(!report.died_quietly);
}

#[test]
fn another_projects_tasks_are_not_counted() {
    let dir = brain();
    seed(dir.path());
    with_tasks(
        dir.path(),
        "T1,Draft,not_started,work__something-else,2026-01-01,2026-01-01,\n",
    );

    let report = show(dir.path(), "work__apply-to-conference", today()).expect("show");

    assert!(report.open_tasks.is_empty());
}
