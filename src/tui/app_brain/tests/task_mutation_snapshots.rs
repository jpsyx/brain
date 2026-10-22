use super::*;

#[test]
fn delayed_settlement_preserves_newer_authoritative_refresh() {
    let tmp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&tmp, &cli, AgentKind::Claude);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let replies = Arc::new(Mutex::new(VecDeque::new()));
    app.services.replace_removal_service(Box::new(Controlled {
        calls: calls.clone(),
        replies: replies.clone(),
    }));
    let original = crate::tasks::task::test_task("T1", "not_started");
    app.tasks.replace_rows(vec![original.clone()], vec![]);
    let identity = app
        .tasks
        .completion_identity("T1", app.context.workspace().id())
        .unwrap()
        .0;
    app.queue_completion(identity, chrono::Local::now().date_naive());
    let id = calls.lock().unwrap()[0].0;
    let mut edited = original.clone();
    edited.name = "Newer external edit".into();
    app.tasks.replace_rows(vec![edited], vec![]);
    replies.lock().unwrap().push_back((
        id,
        Response::Settled(
            Box::new(Response::Complete(Err("synthetic failure".into()))),
            Ok((vec![original], vec![])),
        ),
    ));
    app.tick_removal();
    assert_eq!(
        app.tasks
            .completion_identity("T1", app.context.workspace().id())
            .unwrap()
            .0
            .task_name,
        "Newer external edit"
    );
    assert_eq!(app.services.pending_task_changes(), 0);
    assert_eq!(
        calls.lock().unwrap().len(),
        2,
        "Rejected snapshots must request fresh asynchronous evidence"
    );
}

#[test]
fn fenced_commit_refreshes_derived_rows_without_resurrecting_external_deletion() {
    let tmp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&tmp, &cli, AgentKind::Claude);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let replies = Arc::new(Mutex::new(VecDeque::new()));
    app.services.replace_removal_service(Box::new(Controlled {
        calls: calls.clone(),
        replies: replies.clone(),
    }));
    let habit = crate::tasks::task::test_task("H1", "not_started");
    let original = crate::tasks::task::test_task("T1", "not_started");
    app.tasks
        .replace_rows(vec![original.clone()], vec![habit.clone()]);
    let identity = app
        .tasks
        .completion_identity("H1", app.context.workspace().id())
        .unwrap()
        .0;
    app.queue_completion(identity, chrono::Local::now().date_naive());
    let job = calls.lock().unwrap()[0].0;
    app.tasks.replace_rows(vec![], vec![habit.clone()]);
    let mut done = habit;
    done.status = "done".into();
    let next = crate::tasks::task::test_task("H2", "not_started");
    replies.lock().unwrap().push_back((
        job,
        Response::Settled(
            Box::new(Response::Complete(Ok(
                crate::tasks::complete::CompletionResult {
                    kind: crate::tasks::complete::CompletionKind::Habit,
                    task_id: "H1".into(),
                    task_name: "Habit".into(),
                    next_id: Some("H2".into()),
                    next_due: None,
                    mit_migrated_to: None,
                    project: None,
                    linear_issue: None,
                },
            ))),
            Ok((vec![original], vec![done.clone(), next.clone()])),
        ),
    ));
    app.tick_removal();
    assert!(
        app.tasks
            .completion_identity("T1", app.context.workspace().id())
            .is_none()
    );
    assert!(
        app.tasks
            .completion_identity("H1", app.context.workspace().id())
            .unwrap()
            .1
    );
    assert_eq!(app.services.pending_task_changes(), 0);
    let refresh = calls.lock().unwrap()[1].0;
    assert!(matches!(calls.lock().unwrap()[1].1, Request::Refresh));
    replies.lock().unwrap().push_back((
        refresh,
        Response::Settled(
            Box::new(Response::Refreshed),
            Ok((vec![], vec![done, next])),
        ),
    ));
    app.tick_removal();
    assert!(
        app.tasks
            .completion_identity("T1", app.context.workspace().id())
            .is_none()
    );
    assert!(
        app.tasks
            .completion_identity("H2", app.context.workspace().id())
            .is_some()
    );
    assert!(app.task_changes_allow_quit());
}

#[test]
fn reopened_completion_yields_to_fresh_fifo_evidence_after_delayed_settlement() {
    for intervening_refresh in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&tmp, &cli, AgentKind::Claude);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let replies = Arc::new(Mutex::new(VecDeque::new()));
        app.services.replace_removal_service(Box::new(Controlled {
            calls: calls.clone(),
            replies: replies.clone(),
        }));
        let row = crate::tasks::task::test_task("T1", "not_started");
        app.tasks.replace_rows(vec![row.clone()], vec![]);
        let identity = app
            .tasks
            .completion_identity("T1", app.context.workspace().id())
            .unwrap()
            .0;
        app.queue_completion(identity, chrono::Local::now().date_naive());
        let job = calls.lock().unwrap()[0].0;
        // The worker committed, then another writer reopened the same UUID before delivery.
        if intervening_refresh {
            app.tasks.replace_rows(vec![row.clone()], vec![]);
        }
        replies.lock().unwrap().push_back((
            job,
            Response::Settled(
                Box::new(Response::Complete(Ok(
                    crate::tasks::complete::CompletionResult {
                        kind: crate::tasks::complete::CompletionKind::Task,
                        task_id: "T1".into(),
                        task_name: "Task".into(),
                        next_id: None,
                        next_due: None,
                        mit_migrated_to: None,
                        project: None,
                        linear_issue: None,
                    },
                ))),
                Ok((vec![row.clone()], vec![])),
            ),
        ));
        app.tick_removal();
        if intervening_refresh {
            assert!(
                app.tasks
                    .completion_identity("T1", app.context.workspace().id())
                    .unwrap()
                    .1,
                "A fenced snapshot must not release the commit overlay using older evidence"
            );
            let refresh = calls.lock().unwrap()[1].0;
            replies.lock().unwrap().push_back((
                refresh,
                Response::Settled(
                    Box::new(Response::Refreshed),
                    Ok((vec![row.clone()], vec![])),
                ),
            ));
            app.tick_removal();
        }
        assert!(
            !app.tasks
                .completion_identity("T1", app.context.workspace().id())
                .unwrap()
                .1,
            "Fresh post-commit evidence must reveal the legitimate reopen"
        );
        app.execute_command(Command::Task(TaskCommand::MarkComplete));
        assert!(matches!(app.overlay, Some(Overlay::TaskConfirmation(_))));
    }
}
