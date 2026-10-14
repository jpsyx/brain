use std::sync::{Arc, mpsc};
use std::time::Duration;

use super::*;
use crate::workspace::{CommandContext, RegistryStore};

pub(super) fn writer_fixture() -> (tempfile::TempDir, WorkspaceContext, Vec<FileChange>) {
    let (temp, workspace, _) = fixture();
    crate::project::create(
        workspace.root(),
        "work__sample",
        "Sample",
        "not-started",
        "p2",
        "none",
        "",
    )
    .unwrap();
    let csv = workspace.root().join("tasks/tasks.csv");
    std::fs::write(&csv, "task_id,task_name,project\nT1,Sample,work__sample\n").unwrap();
    std::fs::write(
        workspace.root().join("tasks/habits.csv"),
        "task_id,task_name,project\n",
    )
    .unwrap();
    let meta = workspace
        .root()
        .join("projects/work__sample/.METADATA.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&meta).unwrap()).unwrap();
    value["tasks"] = serde_json::json!(["T1"]);
    std::fs::write(&meta, serde_json::to_vec(&value).unwrap()).unwrap();
    let before = std::fs::read(&meta).unwrap();
    value["tasks"] = serde_json::json!([]);
    let changes = vec![
        FileChange {
            path: csv.clone(),
            before: Some(std::fs::read(csv).unwrap()),
            after: b"task_id,task_name,project\n".to_vec(),
        },
        FileChange {
            path: meta,
            before: Some(before),
            after: serde_json::to_vec(&value).unwrap(),
        },
    ];
    (temp, workspace, changes)
}

#[test]
fn project_writer_waits_for_removal_group() {
    assert_writer_waits(false);
}

#[test]
fn csv_sync_publication_waits_for_removal_group() {
    assert_writer_waits(true);
}

fn assert_writer_waits(sync: bool) {
    let (_temp, workspace, changes) = writer_fixture();
    let (paused_tx, paused_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let removal_workspace = workspace.clone();
    let removal = std::thread::spawn(move || {
        let owner = TaskStoreOwner::acquire(&removal_workspace).unwrap();
        replace_group_with_hook(
            &removal_workspace,
            &owner,
            TargetPolicy::TaskRemoval,
            &changes,
            |step| {
                if step == TransactionStep::Install(1) {
                    paused_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }
                Ok(())
            },
        )
        .unwrap();
    });
    paused_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let writer_workspace = workspace.clone();
    let writer = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        if sync {
            crate::sync::csv_sync::sync_csvs_with_transport(
                &writer_workspace,
                crate::sync::args::Direction::Both,
                |_| None,
                |_, _| true,
            )
            .unwrap();
        } else {
            let context = CommandContext::for_test(
                Arc::new(writer_workspace),
                RegistryStore::from_path(std::path::PathBuf::from("unused")),
                "member",
            );
            crate::command::project::run(
                &crate::cli::ProjectArgs {
                    action: crate::cli::ProjectAction::Set(crate::cli::ProjectSetArgs {
                        slug: "work__sample".into(),
                        title: Some("Updated".into()),
                        status: None,
                        priority: None,
                        due: None,
                    }),
                },
                &context,
            )
            .unwrap();
        }
        done_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let finished_early = done_rx.recv_timeout(Duration::from_millis(200)).is_ok();
    release_tx.send(()).unwrap();
    removal.join().unwrap();
    writer.join().unwrap();
    assert!(
        !finished_early,
        "canonical writer ran while removal was half-installed"
    );
    let meta: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            workspace
                .root()
                .join("projects/work__sample/.METADATA.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(meta["tasks"], serde_json::json!([]));
    if !sync {
        assert_eq!(meta["title"], "Updated");
    }
}

#[test]
fn acquiring_native_writer_recovers_interrupted_removal() {
    let (_temp, workspace, changes) = writer_fixture();
    {
        let owner = TaskStoreOwner::acquire(&workspace).unwrap();
        crash_at(&workspace, &owner, &changes, TransactionStep::Install(1));
    }
    let _owner = TaskStoreOwner::acquire(&workspace).unwrap();
    assert_old(&changes);
}

#[test]
fn habits_http_recovers_before_completion() {
    let (_temp, workspace, _) = writer_fixture();
    let habits = workspace.root().join("tasks/habits.csv");
    let before = b"task_id,task_name,status,due_date,recur_interval,recur_unit\nH1,Walk,not_started,2026-10-14,1,days\n";
    std::fs::write(&habits, before).unwrap();
    let changes = [FileChange {
        path: habits,
        before: Some(before.to_vec()),
        after: b"task_id,task_name,status,due_date,recur_interval,recur_unit\n".to_vec(),
    }];
    {
        let owner = TaskStoreOwner::acquire(&workspace).unwrap();
        crash_at(
            &workspace,
            &owner,
            &changes,
            TransactionStep::SyncInstall(0),
        );
    }
    let result = crate::server::routes::habits::done(
        &RegistryStore::from_path(workspace.root().join("unused.json")),
        &workspace,
        r#"{"task_id":"H1"}"#,
    );
    assert!(
        matches!(
            result,
            crate::server::routes::habits::DoneOutcome::Done { .. }
        ),
        "{result:?}"
    );
}

#[test]
fn removal_owned_snapshot_recovers_before_reading() {
    let (_temp, workspace, changes) = writer_fixture();
    std::fs::write(workspace.root().join(".config/config.json"), "{}\n").unwrap();
    let owner = TaskStoreOwner::acquire(&workspace).unwrap();
    crash_at(&workspace, &owner, &changes, TransactionStep::Install(1));
    let actor = crate::actor::test_actor("member");
    let integration = crate::tasks::removal::model::IntegrationSnapshot::default();
    let snapshot = crate::tasks::removal::snapshot::Snapshot::read_owned(
        &crate::tasks::removal::snapshot::SnapshotContext {
            workspace: &workspace,
            actor: &actor,
            integration: &integration,
        },
        chrono::NaiveDate::from_ymd_opt(2026, 10, 14).unwrap(),
        &owner,
    )
    .unwrap();
    assert_eq!(
        snapshot.tasks.parsed.rows.len(),
        1,
        "owned snapshot read a half-installed removal"
    );
}
