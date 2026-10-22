use super::*;

#[test]
fn recovered_commit_with_refresh_failure_does_not_revive_removed_target() {
    let temp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temp, &cli, AgentKind::Claude);
    app.tasks.replace_rows(
        vec![crate::tasks::task::test_task("T1", "not_started")],
        vec![],
    );
    let identity = app
        .tasks
        .completion_identity("T1", app.context.workspace().id())
        .unwrap()
        .0;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let replies = Arc::new(Mutex::new(VecDeque::new()));
    app.services.replace_removal_service(Box::new(Controlled {
        calls: calls.clone(),
        replies: replies.clone(),
    }));
    app.run_remove("T1");
    let preview = calls.lock().unwrap()[0].0;
    let mut captured = plan();
    captured.targets = vec![identity.clone()];
    replies
        .lock()
        .unwrap()
        .push_back((preview, Response::Preview(Ok(captured))));
    app.tick_removal();
    app.confirm_removal();
    let apply = calls.lock().unwrap()[1].0;
    replies.lock().unwrap().push_back((
        apply,
        Response::Receipt(Err(crate::tasks::removal::model::Problem::new(
            "external_pending",
            "Check issue",
            serde_json::json!({"operation_id":"saved-operation"}),
        ))),
    ));
    app.tick_removal();
    assert!(
        app.tasks
            .completion_identity("T1", app.context.workspace().id())
            .is_some()
    );
    app.reopen_removal();
    for code in [KeyCode::Down, KeyCode::Down, KeyCode::Enter] {
        crate::tui::task_removal::handle_key(&mut app, &KeyEvent::new(code, KeyModifiers::NONE));
    }
    let resume = calls.lock().unwrap()[2].0;
    replies.lock().unwrap().push_back((
        resume,
        Response::Settled(
            Box::new(Response::Receipt(Ok(Receipt {
                operation_id: "saved-operation".into(),
                committed: true,
                removed: vec![identity],
                updated: vec![],
                external: vec![],
                warnings: vec![],
            }))),
            Err("refresh unavailable".into()),
        ),
    ));
    app.tick_removal();
    assert!(
        app.tasks
            .completion_identity("T1", app.context.workspace().id())
            .is_none()
    );
    assert!(app.status.error().unwrap().contains("refresh unavailable"));
}

#[test]
fn accepted_results_settle_independently_and_keep_older_recovery() {
    let tmp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&tmp, &cli, AgentKind::Claude);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let replies = Arc::new(Mutex::new(VecDeque::new()));
    app.services.replace_removal_service(Box::new(Controlled {
        calls: calls.clone(),
        replies: replies.clone(),
    }));
    let mut jobs = Vec::new();
    for target in ["T1", "T2"] {
        app.run_remove(target);
        let id = calls.lock().unwrap().last().unwrap().0;
        replies
            .lock()
            .unwrap()
            .push_back((id, Response::Preview(Ok(plan()))));
        app.tick_removal();
        app.confirm_removal();
        jobs.push(calls.lock().unwrap().last().unwrap().0);
        assert!(app.overlay.is_none());
    }
    assert_eq!(app.services.pending_task_changes(), 2);
    app.execute_command(Command::Global(
        crate::tui::action::GlobalAction::ShowShortcuts,
    ));
    replies.lock().unwrap().push_back((
        jobs[0],
        Response::Receipt(Err(crate::tasks::removal::model::Problem::new(
            "external_pending",
            "Check issue",
            serde_json::json!({"operation_id":"older-recovery"}),
        ))),
    ));
    replies.lock().unwrap().push_back((
        jobs[1],
        Response::Receipt(Ok(Receipt {
            operation_id: "newer-success".into(),
            committed: true,
            removed: vec![],
            updated: vec![],
            external: vec![],
            warnings: vec![],
        })),
    ));
    app.tick_removal();
    assert_eq!(app.services.pending_task_changes(), 0);
    assert!(matches!(app.overlay, Some(Overlay::Help(_))));
    assert!(app.status.error().unwrap().contains("T1"));
    assert_eq!(
        app.services
            .recoverable_removal()
            .unwrap()
            .operation_id
            .as_deref(),
        Some("older-recovery")
    );
}

#[test]
fn failures_before_operation_creation_do_not_trap_recovery() {
    let tmp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&tmp, &cli, AgentKind::Claude);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let replies = Arc::new(Mutex::new(VecDeque::new()));
    app.services.replace_removal_service(Box::new(Controlled {
        calls: calls.clone(),
        replies: replies.clone(),
    }));
    app.run_remove("T1");
    let id = calls.lock().unwrap()[0].0;
    replies
        .lock()
        .unwrap()
        .push_back((id, Response::Preview(Ok(plan()))));
    app.tick_removal();
    app.confirm_removal();
    let id = calls.lock().unwrap()[1].0;
    replies.lock().unwrap().push_back((
        id,
        Response::Receipt(Err(crate::tasks::removal::model::Problem::new(
            "snapshot_failed",
            "Could not inspect files",
            serde_json::json!({}),
        ))),
    ));
    app.tick_removal();
    assert!(app.services.recoverable_removal().is_none());
    app.run_remove("T1");
    assert!(matches!(
        calls.lock().unwrap()[2].1,
        Request::Preview { .. }
    ));
}

#[test]
fn failed_status_preserves_existing_recovery_when_remove_reopens() {
    let tmp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&tmp, &cli, AgentKind::Claude);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let replies = Arc::new(Mutex::new(VecDeque::new()));
    app.services.replace_removal_service(Box::new(Controlled {
        calls: calls.clone(),
        replies: replies.clone(),
    }));
    app.run_remove("T1");
    let id = calls.lock().unwrap()[0].0;
    replies
        .lock()
        .unwrap()
        .push_back((id, Response::Preview(Ok(plan()))));
    app.tick_removal();
    app.confirm_removal();
    let id = calls.lock().unwrap()[1].0;
    replies.lock().unwrap().push_back((
        id,
        Response::Receipt(Err(crate::tasks::removal::model::Problem::new(
            "external_pending",
            "Check issue",
            serde_json::json!({"operation_id":"saved-operation"}),
        ))),
    ));
    app.tick_removal();
    let state = app.services.recoverable_removal().unwrap().clone();
    app.removal_request(state, Request::Status("saved-operation".into()));
    let id = calls.lock().unwrap()[2].0;
    replies.lock().unwrap().push_back((
        id,
        Response::Operation(Err(crate::tasks::removal::model::Problem::new(
            "recovery_required",
            "Try later",
            serde_json::json!({}),
        ))),
    ));
    app.tick_removal();
    app.overlay = None;
    app.run_remove("T1");
    assert_eq!(
        calls.lock().unwrap().len(),
        3,
        "Remove must reopen saved recovery rather than start a new preview"
    );
    assert!(
        matches!(&app.overlay, Some(Overlay::TaskRemoval(state)) if state.operation_id.as_deref() == Some("saved-operation"))
    );
}

#[path = "task_mutation_snapshots.rs"]
mod snapshots;
