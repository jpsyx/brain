use super::external::{calls, fixture, progress, rows, start};
use crate::tasks::removal::{abandon, operations::Phase, resume};

#[test]
fn removal_external_network_calls_release_task_ownership_and_recheck_local_edits() {
    let f = fixture(
        r"import sqlite3
# The real lock path is supplied by the test, not guessed by the adapter.
lock=pathlib.Path((base/'lock-path').read_text())
connection=sqlite3.connect(str(lock),timeout=0)
connection.execute('BEGIN IMMEDIATE')
connection.rollback()
if r['action']=='cancel' and r['issue']=='EX-2':
    tasks=base/'workspace/tasks/tasks.csv'
    tasks.write_text(tasks.read_text().replace('First','Edited during network'))",
    );
    std::fs::write(
        f.temporary.path().join("lock-path"),
        f.workspace
            .paths()
            .task_store_lock()
            .to_string_lossy()
            .as_bytes(),
    )
    .unwrap();
    let error = start(&f).unwrap_err();
    assert_eq!(error.code, "stale_plan");
    let id = error.details["operation_id"].as_str().unwrap();
    assert!(rows(&f).contains("Edited during network"));
    assert!(rows(&f).contains("T18"));
    assert_eq!(progress(&f, id)["phase"], "needs_reconfirmation");
    assert_eq!(progress(&f, id)["external"][1]["state"], "succeeded");
    assert_eq!(abandon(&f.command(), id).unwrap().phase, Phase::Abandoned);
}

#[test]
fn removal_external_network_stale_rows_allow_status_only_then_abandon() {
    let f = fixture(
        r"import sqlite3
connection=sqlite3.connect((base/'lock-path').read_text(),timeout=0)
connection.execute('BEGIN IMMEDIATE')
connection.rollback()
if r['action']=='cancel': status='error'
if r['action']=='status': status='canceled'",
    );
    std::fs::write(
        f.temporary.path().join("lock-path"),
        f.workspace
            .paths()
            .task_store_lock()
            .to_string_lossy()
            .as_bytes(),
    )
    .unwrap();
    let error = start(&f).unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    f.write(
        "tasks/tasks.csv",
        b"task_id,task_name,linear_issue\nT17,Edited,EX-1\nT18,Second,EX-2\n",
    );
    let before = rows(&f);
    assert_eq!(resume(&f.command(), id).unwrap_err().code, "stale_plan");
    assert_eq!(progress(&f, id)["external"][0]["state"], "succeeded");
    assert_eq!(calls(&f).matches("cancel EX-1\n").count(), 1);
    assert_eq!(calls(&f).matches("cancel EX-2\n").count(), 0);
    assert_eq!(abandon(&f.command(), id).unwrap().phase, Phase::Abandoned);
    assert_eq!(rows(&f), before);
}

#[test]
fn removal_external_network_exact_restoration_can_resume_without_recanceling() {
    let f = fixture(
        "if r['action']=='cancel' and r['issue']=='EX-2':\n    tasks=base/'workspace/tasks/tasks.csv'\n    tasks.write_text(tasks.read_text().replace('First','Edited'))",
    );
    let original = rows(&f);
    let error = start(&f).unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    assert_eq!(progress(&f, id)["phase"], "needs_reconfirmation");
    let before_calls = calls(&f);
    f.write("tasks/tasks.csv", original.as_bytes());
    assert!(resume(&f.command(), id).unwrap().committed);
    assert_eq!(calls(&f), before_calls);
    assert_eq!(rows(&f).lines().count(), 1);
}
