use super::*;
use crate::tasks::removal::{
    model::{Options, RemovalPlan},
    operations::Receipt,
};
use crate::tui::overlay::Overlay;
use crate::tui::palette::{Command, TaskCommand};
use crate::tui::task_removal::{
    RemovalScreen,
    worker::{RemovalService, Request, Response},
};
use crossterm::event::{KeyEvent, KeyModifiers};
use std::collections::{BTreeMap, VecDeque};

struct NativeRecorder {
    calls: Arc<Mutex<Vec<Request>>>,
    results: VecDeque<(u64, Response)>,
}
impl RemovalService for NativeRecorder {
    fn start(&mut self, _: CommandContext, generation: u64, request: Request) {
        self.calls.lock().unwrap().push(request.clone());
        let response = match request {
            Request::Preview { options, .. } => Response::Preview(Ok(RemovalPlan {
                presentation_labels: std::collections::BTreeMap::new(),
                maintenance: None,
                fingerprint: "saved-consent".into(),
                targets: vec![],
                options,
                edits: vec![],
                external: vec![],
                successors: BTreeMap::new(),
                blockers: vec![],
                warnings: vec![],
            })),
            Request::Apply { plan, expected, .. } => {
                assert_eq!(expected, "saved-consent");
                assert_eq!(plan.fingerprint, expected);
                Response::Receipt(Ok(Receipt {
                    operation_id: "operation-1".into(),
                    committed: true,
                    removed: vec![],
                    updated: vec![],
                    external: vec![],
                    warnings: vec![],
                }))
            }
            _ => panic!("unexpected request"),
        };
        self.results.push_back((generation, response));
    }
    fn poll(&mut self) -> Option<(u64, Response)> {
        self.results.pop_front()
    }
}

#[test]
fn native_task_removal_invokes_native_service_without_a_brain_prompt() {
    for unavailable in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let cli = Cli::parse_from(["tasks"]);
        let mut app = test_app(&tmp, &cli, AgentKind::Claude);
        let recording = TransportRecording::default();
        app.brain.replace_brain_transport(recording.transport());
        if unavailable {
            app.context = app
                .context
                .replacing_agent_command_for_test("/unavailable-agent");
        }
        app.tasks.replace_rows(
            vec![crate::tasks::task::test_task("T1", "not_started")],
            vec![],
        );
        let calls = Arc::new(Mutex::new(Vec::new()));
        app.services
            .replace_removal_service(Box::new(NativeRecorder {
                calls: calls.clone(),
                results: VecDeque::new(),
            }));
        let before = (app.shell.main_view(), app.shell.focus());
        app.execute_command(Command::Task(TaskCommand::Remove));
        app.tick_removal();
        assert!(
            matches!(app.overlay, Some(Overlay::TaskRemoval(ref state)) if matches!(state.screen, RemovalScreen::Preview(_)))
        );
        crate::tui::task_removal::handle_key(
            &mut app,
            &KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        );
        crate::tui::task_removal::handle_key(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
        app.tick_removal();
        assert!(matches!(
            calls.lock().unwrap().as_slice(),
            [
                Request::Preview {
                    options: Options { habit: false, .. },
                    ..
                },
                Request::Apply { .. }
            ]
        ));
        assert!(recording.launch_specs().is_empty());
        assert!(recording.inputs().is_empty());
        assert_eq!((app.shell.main_view(), app.shell.focus()), before);
    }
}

#[test]
fn native_task_removal_dependency_choice_rebuilds_without_applying() {
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
    let mut preview = plan();
    preview
        .blockers
        .push(crate::tasks::removal::model::Problem::new(
            "dependents_exist",
            "T2 needs T1",
            serde_json::json!({"task_id":"T2"}),
        ));
    replies
        .lock()
        .unwrap()
        .push_back((generation, Response::Preview(Ok(preview))));
    app.tick_removal();
    // Cancel is first; the next row chooses how waiting tasks should behave.
    for code in [KeyCode::Down, KeyCode::Enter] {
        crate::tui::task_removal::handle_key(&mut app, &KeyEvent::new(code, KeyModifiers::NONE));
    }
    let calls = calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert!(matches!(&calls[1].1, Request::Preview { options, .. }
        if options.dependents == crate::tasks::removal::model::Dependents::Rewire));
}

struct Controlled {
    calls: Arc<Mutex<Vec<(u64, Request)>>>,
    replies: Arc<Mutex<VecDeque<(u64, Response)>>>,
}
impl RemovalService for Controlled {
    fn start(&mut self, _: CommandContext, generation: u64, request: Request) {
        self.calls.lock().unwrap().push((generation, request));
    }
    fn poll(&mut self) -> Option<(u64, Response)> {
        self.replies.lock().unwrap().pop_front()
    }
}
fn plan() -> RemovalPlan {
    RemovalPlan {
        presentation_labels: std::collections::BTreeMap::new(),
        maintenance: None,
        fingerprint: "consent".into(),
        targets: vec![],
        options: Options::default(),
        edits: vec![],
        external: vec![],
        successors: BTreeMap::new(),
        blockers: vec![],
        warnings: vec![],
    }
}
#[test]
fn native_task_removal_fences_late_previews_and_retains_closed_execution_recovery() {
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
    let first = calls.lock().unwrap()[0].0;
    crate::tui::task_removal::handle_key(
        &mut app,
        &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    );
    app.run_remove("T2");
    let second = calls.lock().unwrap()[1].0;
    replies
        .lock()
        .unwrap()
        .push_back((first, Response::Preview(Ok(plan()))));
    app.tick_removal();
    assert!(
        matches!(app.overlay, Some(Overlay::TaskRemoval(ref state)) if matches!(state.screen, RemovalScreen::Running { .. }) && state.targets == ["T2"])
    );
    replies
        .lock()
        .unwrap()
        .push_back((second, Response::Preview(Ok(plan()))));
    app.tick_removal();
    crate::tui::task_removal::handle_key(
        &mut app,
        &KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    );
    crate::tui::task_removal::handle_key(
        &mut app,
        &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    let third = calls.lock().unwrap()[2].0;
    // Running admits no second execution, even after close/reopen.
    for _ in 0..2 {
        for code in [
            KeyCode::Enter,
            KeyCode::Char('r'),
            KeyCode::Char('s'),
            KeyCode::Char('a'),
            KeyCode::Char('y'),
            KeyCode::Char('p'),
        ] {
            crate::tui::task_removal::handle_key(
                &mut app,
                &KeyEvent::new(code, KeyModifiers::NONE),
            );
        }
        crate::tui::task_removal::handle_key(
            &mut app,
            &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );
        app.run_remove("T3");
        assert_eq!(calls.lock().unwrap().len(), 3);
    }
    crate::tui::task_removal::handle_key(
        &mut app,
        &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    );
    replies.lock().unwrap().push_back((
        third,
        Response::Receipt(Err(crate::tasks::removal::model::Problem::new(
            "external_pending",
            "uncertain",
            serde_json::json!({"operation_id":"saved-operation"}),
        ))),
    ));
    app.tick_removal();
    assert!(app.overlay.is_none());
    app.tasks.replace_rows(vec![], vec![]);
    app.execute_command(Command::Task(TaskCommand::Remove));
    assert_eq!(calls.lock().unwrap().len(), 3);
    assert!(
        matches!(app.overlay, Some(Overlay::TaskRemoval(ref state)) if state.operation_id.as_deref() == Some("saved-operation"))
    );
    for code in [KeyCode::Up, KeyCode::Enter] {
        crate::tui::task_removal::handle_key(&mut app, &KeyEvent::new(code, KeyModifiers::NONE));
    }
    assert_eq!(calls.lock().unwrap().len(), 3);
    for code in [KeyCode::Down, KeyCode::Enter] {
        crate::tui::task_removal::handle_key(&mut app, &KeyEvent::new(code, KeyModifiers::NONE));
    }
    assert!(matches!(&calls.lock().unwrap()[3].1, Request::Abandon(id) if id == "saved-operation"));
    for code in [
        KeyCode::Char('s'),
        KeyCode::Char('r'),
        KeyCode::Char('a'),
        KeyCode::Char('y'),
    ] {
        crate::tui::task_removal::handle_key(&mut app, &KeyEvent::new(code, KeyModifiers::NONE));
    }
    assert_eq!(calls.lock().unwrap().len(), 4);
}
#[test]
fn native_task_removal_stale_preview_rebuilds_without_confirming_and_confirmation_reads_fresh_date()
{
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
    let first = calls.lock().unwrap()[0].0;
    replies
        .lock()
        .unwrap()
        .push_back((first, Response::Preview(Ok(plan()))));
    app.tick_removal();
    crate::tui::task_removal::handle_key(
        &mut app,
        &KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
    );
    crate::tui::task_removal::handle_key(
        &mut app,
        &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    let second = calls.lock().unwrap()[1].0;
    assert!(
        matches!(&calls.lock().unwrap()[1].1, Request::Apply { today, expected, .. } if *today == chrono::Local::now().date_naive() && expected == "consent")
    );
    replies.lock().unwrap().push_back((
        second,
        Response::Receipt(Err(crate::tasks::removal::model::Problem::new(
            "stale_plan",
            "changed",
            serde_json::json!({}),
        ))),
    ));
    app.tick_removal();
    assert!(matches!(
        &calls.lock().unwrap()[2].1,
        Request::Preview { .. }
    ));
    let third = calls.lock().unwrap()[2].0;
    replies
        .lock()
        .unwrap()
        .push_back((third, Response::Preview(Ok(plan()))));
    app.tick_removal();
    assert_eq!(calls.lock().unwrap().len(), 3);
    assert!(
        matches!(app.overlay, Some(Overlay::TaskRemoval(ref state)) if matches!(state.screen, RemovalScreen::Preview(_)) && !state.notice.is_empty())
    );
}

#[test]
fn native_task_removal_keeps_task_only_picker_and_renders_abandon_consequence() {
    let tmp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&tmp, &cli, AgentKind::Claude);
    app.tasks.replace_rows(
        vec![crate::tasks::task::test_task("T1", "not_started")],
        vec![crate::tasks::task::test_task("H1", "not_started")],
    );
    app.shell
        .show_main_view(crate::main_view::MainView::BrainSearch);
    app.execute_command(Command::Task(TaskCommand::Remove));
    assert!(matches!(app.overlay, Some(Overlay::TaskTargetPicker(_))));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let replies = Arc::new(Mutex::new(VecDeque::new()));
    app.services.replace_removal_service(Box::new(Controlled {
        calls: calls.clone(),
        replies: replies.clone(),
    }));
    crate::tui::handlers::handle_task_target_picker_key(
        &mut app,
        &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    let generation = calls.lock().unwrap()[0].0;
    assert!(
        matches!(&calls.lock().unwrap()[0].1, Request::Preview { targets, options, .. } if targets == &["T1"] && !options.habit)
    );
    replies.lock().unwrap().push_back((
        generation,
        Response::Preview(Err(crate::tasks::removal::model::Problem::new(
            "external_pending",
            "uncertain",
            serde_json::json!({"operation_id":"recover-me"}),
        ))),
    ));
    app.tick_removal();
    for code in [KeyCode::Up, KeyCode::Enter] {
        crate::tui::task_removal::handle_key(&mut app, &KeyEvent::new(code, KeyModifiers::NONE));
    }
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(110, 30)).unwrap();
    terminal
        .draw(|frame| crate::tui::draw::draw(frame, &mut app))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(text.contains("The task stays in Brain."));
    assert!(
        text.contains("Any linked issues already canceled stay"),
        "{text}"
    );
    assert!(text.contains("canceled."), "{text}");
    assert_eq!(
        app.shell.main_view(),
        crate::main_view::MainView::BrainSearch
    );
}

#[test]
fn native_task_removal_enter_defaults_to_cancel() {
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
    for code in [KeyCode::Char('y'), KeyCode::Char('i')] {
        crate::tui::task_removal::handle_key(&mut app, &KeyEvent::new(code, KeyModifiers::NONE));
    }
    assert!(app.overlay.is_some());
    assert_eq!(calls.lock().unwrap().len(), 1);
    crate::tui::task_removal::handle_key(
        &mut app,
        &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    assert!(app.overlay.is_none());
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[path = "native_task_removal_hotkey.rs"]
mod hotkey;
