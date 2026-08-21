// Auto-healing a remote that carries current task CSVs but no schema document.

use std::cell::RefCell;

const CURRENT_TASKS: &str = "task_uuid,task_id,task_name,assigned_to,system_key\n";
const CURRENT_HABITS: &str = "task_uuid,task_id,task_name,assigned_to,system_key\n";
const CURRENT_DOCUMENT: &str = r#"{"task_schema_version":2,"merge_key":"task_uuid","display_identity":{"field":"task_id","mutable":true}}"#;

fn current_workspace(root: &std::path::Path) {
    let tasks = root.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    std::fs::write(tasks.join("tasks.csv"), CURRENT_TASKS).unwrap();
    std::fs::write(tasks.join("habits.csv"), CURRENT_HABITS).unwrap();
    std::fs::write(tasks.join("SCHEMA.json"), CURRENT_DOCUMENT).unwrap();
}

/// The `~/family` dead end: seeding gave the local workspace a current schema
/// document, the remote never received one (bisync excludes it and only setup
/// published it), and the remote's CSVs are header-only current. Refusing left
/// no command that worked — not `brain sync`, not `brain sync setup`. Brain
/// publishes the document instead, which is what setup would have done.
#[test]
fn an_absent_remote_document_over_current_remote_csvs_is_published_and_the_sync_proceeds() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    current_workspace(&root);
    let pushed = RefCell::new(Vec::<(String, String)>::new());

    let result = sync_csvs_with_transport(
        &paths(directory.path()),
        &root,
        Direction::Both,
        |relative: &str| match relative {
            "tasks/tasks.csv" => Some(CURRENT_TASKS.to_owned()),
            "tasks/habits.csv" => Some(CURRENT_HABITS.to_owned()),
            _ => None,
        },
        |relative: &str, body: &str| {
            pushed
                .borrow_mut()
                .push((relative.to_owned(), body.to_owned()));
            true
        },
    );

    assert!(result.is_ok(), "{:#}", result.unwrap_err());
    let pushed = pushed.into_inner();
    let document = pushed
        .iter()
        .find(|(relative, _)| relative == "tasks/SCHEMA.json")
        .expect("the missing schema document must be published");
    assert_eq!(document.1, CURRENT_DOCUMENT);
}

/// The guard that must survive: real legacy rows on the remote still refuse,
/// because publishing a current document over them would misdescribe the data.
#[test]
fn an_absent_remote_document_over_legacy_remote_csvs_refuses_and_names_the_remedy() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    current_workspace(&root);
    let pushed = RefCell::new(Vec::<String>::new());

    let error = sync_csvs_with_transport(
        &paths(directory.path()),
        &root,
        Direction::Both,
        |relative: &str| match relative {
            "tasks/tasks.csv" => Some("task_id,status\nT1,open\n".to_owned()),
            "tasks/habits.csv" => Some("task_id,status\nH1,open\n".to_owned()),
            _ => None,
        },
        |relative: &str, _body: &str| {
            pushed.borrow_mut().push(relative.to_owned());
            true
        },
    )
    .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("legacy"), "{message}");
    assert!(
        message.contains("workspace migrate"),
        "the refusal must name the remedy: {message}"
    );
    assert!(
        pushed.into_inner().is_empty(),
        "nothing may be published over legacy remote rows"
    );
}

/// An entirely empty remote is initialized too: the document is published so a
/// brand-new remote converges without a separate setup step.
#[test]
fn an_empty_remote_receives_the_schema_document() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    current_workspace(&root);
    let pushed = RefCell::new(Vec::<String>::new());

    let result = sync_csvs_with_transport(
        &paths(directory.path()),
        &root,
        Direction::Both,
        |_relative: &str| None,
        |relative: &str, _body: &str| {
            pushed.borrow_mut().push(relative.to_owned());
            true
        },
    );

    assert!(result.is_ok(), "{:#}", result.unwrap_err());
    assert!(
        pushed
            .into_inner()
            .iter()
            .any(|relative| relative == "tasks/SCHEMA.json")
    );
}

/// A publish that fails must not be reported as a healed remote.
#[test]
fn a_failed_document_publication_is_surfaced_rather_than_assumed() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    current_workspace(&root);

    let error = sync_csvs_with_transport(
        &paths(directory.path()),
        &root,
        Direction::Both,
        |relative: &str| match relative {
            "tasks/tasks.csv" => Some(CURRENT_TASKS.to_owned()),
            "tasks/habits.csv" => Some(CURRENT_HABITS.to_owned()),
            _ => None,
        },
        |relative: &str, _body: &str| relative != "tasks/SCHEMA.json",
    )
    .unwrap_err();

    assert!(error.to_string().contains("schema document"), "{error:#}");
}
