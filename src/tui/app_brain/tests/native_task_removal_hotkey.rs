use super::*;

#[test]
fn native_task_removal_d_confirms_preview_from_either_selection() {
    for selected in [0, 1] {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&tmp, &cli, AgentKind::Claude);
        let calls = Arc::new(Mutex::new(Vec::new()));
        app.services
            .replace_removal_service(Box::new(NativeRecorder {
                calls: calls.clone(),
                results: VecDeque::new(),
            }));
        app.run_remove("T1");
        app.tick_removal();
        let Some(Overlay::TaskRemoval(state)) = &mut app.overlay else {
            panic!("expected deletion preview");
        };
        state.selected = selected;
        crate::tui::task_removal::handle_key(
            &mut app,
            &KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        );
        assert!(matches!(
            calls.lock().unwrap().as_slice(),
            [Request::Preview { .. }, Request::Apply { expected, .. }]
                if expected == "saved-consent"
        ));
    }
}

#[test]
fn native_task_removal_success_dismisses_confirming_modal() {
    for shortcut in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&tmp, &cli, AgentKind::Claude);
        app.services
            .replace_removal_service(Box::new(NativeRecorder {
                calls: Arc::new(Mutex::new(Vec::new())),
                results: VecDeque::new(),
            }));
        app.run_remove("T1");
        app.tick_removal();
        let keys = if shortcut {
            vec![KeyCode::Char('d')]
        } else {
            vec![KeyCode::Down, KeyCode::Enter]
        };
        for code in keys {
            crate::tui::task_removal::handle_key(
                &mut app,
                &KeyEvent::new(code, KeyModifiers::NONE),
            );
        }
        app.tick_removal();
        assert!(
            app.overlay.is_none(),
            "successful deletion should dismiss its modal"
        );
    }
}

#[test]
fn native_task_removal_d_cannot_confirm_unavailable_deletion() {
    use crate::tasks::removal::model::Problem;
    for unavailable in ["blocked", "running", "recovery", "abandon", "failed"] {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&tmp, &cli, AgentKind::Claude);
        let calls = Arc::new(Mutex::new(Vec::new()));
        app.services
            .replace_removal_service(Box::new(NativeRecorder {
                calls: calls.clone(),
                results: VecDeque::new(),
            }));
        app.run_remove("T1");
        app.tick_removal();
        let Some(Overlay::TaskRemoval(state)) = &mut app.overlay else {
            panic!("expected deletion preview");
        };
        match unavailable {
            "blocked" => {
                let RemovalScreen::Preview(plan) = &mut state.screen else {
                    unreachable!();
                };
                plan.blockers
                    .push(Problem::new("blocked", "blocked", serde_json::json!({})));
            }
            "running" => state.screen = RemovalScreen::Running { operation_id: None },
            "recovery" => state.operation_id = Some("saved-operation".into()),
            "abandon" => state.abandon_confirm = true,
            "failed" => {
                state.screen =
                    RemovalScreen::Failed(Problem::new("failed", "failed", serde_json::json!({})));
            }
            _ => unreachable!(),
        }
        crate::tui::task_removal::handle_key(
            &mut app,
            &KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        );
        assert_eq!(calls.lock().unwrap().len(), 1, "{unavailable}");
        assert!(app.overlay.is_some(), "{unavailable}");
    }
}

#[test]
fn native_task_removal_completion_preserves_details_and_newer_modals() {
    use crate::tasks::removal::model::Problem;
    for case in ["warning", "failure", "newer-modal", "refresh-failure"] {
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
        let generation = calls.lock().unwrap()[0].0;
        replies
            .lock()
            .unwrap()
            .push_back((generation, Response::Preview(Ok(plan()))));
        app.tick_removal();
        crate::tui::task_removal::handle_key(
            &mut app,
            &KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        );
        let generation = calls.lock().unwrap()[1].0;
        let problem = || {
            Problem::new(
                "needs_attention",
                "Review these details",
                serde_json::json!({}),
            )
        };
        let response = if case == "failure" {
            Err(problem())
        } else {
            Ok(Receipt {
                operation_id: "operation-1".into(),
                committed: true,
                removed: vec![],
                updated: vec![],
                external: vec![],
                warnings: if case == "warning" {
                    vec![problem()]
                } else {
                    vec![]
                },
            })
        };
        if case == "newer-modal" {
            app.execute_command(Command::Global(
                crate::tui::action::GlobalAction::ShowShortcuts,
            ));
        }
        let response = Response::Receipt(response);
        let response = if case == "refresh-failure" {
            Response::Settled(Box::new(response), Err("reading tasks.csv failed".into()))
        } else {
            response
        };
        replies.lock().unwrap().push_back((generation, response));
        app.tick_removal();
        if case == "newer-modal" {
            assert!(matches!(app.overlay, Some(Overlay::Help(_))));
        } else {
            assert!(app.overlay.is_none());
            assert!(
                app.status.error().is_some(),
                "{case} must retain details outside the closed modal"
            );
        }
    }
}
