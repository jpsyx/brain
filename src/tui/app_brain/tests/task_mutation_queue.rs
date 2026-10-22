use super::*;
use crate::tui::{
    overlay::Overlay,
    palette::{Command, TaskCommand},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

#[test]
fn completion_closes_projects_and_accepts_input_while_real_lock_is_held() {
    let temp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temp, &cli, AgentKind::Claude);
    let path = app.context.tasks_csv_path().to_path_buf();
    seed(
        &path,
        "task_id,task_name,status\nT1,First,not_started\nT2,Second,not_started\n",
    )
    .unwrap();
    app.reload_tasks().unwrap();
    app.tasks.set_view(crate::tasks::view::View::All);
    app.execute_command(Command::Task(TaskCommand::MarkComplete));
    assert!(matches!(app.overlay, Some(Overlay::TaskConfirmation(_))));
    let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(app.context.workspace()).unwrap();
    let start = Instant::now();
    crate::tui::event_loop::update_application(
        &mut app,
        &Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    assert!(start.elapsed() < Duration::from_millis(200));
    assert!(app.overlay.is_none());
    assert!(
        app.tasks
            .completion_identity("T1", app.context.workspace().id())
            .unwrap()
            .1
    );
    assert!(!crate::tui::event_loop::update_application(
        &mut app,
        &Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL))
    ));
    app.reload_tasks().unwrap();
    assert!(
        app.tasks
            .completion_identity("T1", app.context.workspace().id())
            .unwrap()
            .1
    );
    app.tasks.cycle_view_next();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("not_started")
    );
    drop(owner);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.services.pending_task_changes() > 0 {
        app.tick_removal();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(std::fs::read_to_string(&path).unwrap().contains("done"));
    assert!(crate::tui::event_loop::update_application(
        &mut app,
        &Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL))
    ));
}

fn key(app: &mut App, code: KeyCode) -> bool {
    crate::tui::event_loop::update_application(
        app,
        &Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
    )
}
fn drain(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.services.pending_task_changes() > 0 {
        app.tick_removal();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn wait_preview(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.tick_removal();
        if matches!(&app.overlay,Some(Overlay::TaskRemoval(state)) if matches!(state.screen,crate::tui::task_removal::RemovalScreen::Preview(_)))
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn deletion_closes_projects_and_fifo_preview_observes_prior_completion() {
    let temp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temp, &cli, AgentKind::Claude);
    let path = app.context.tasks_csv_path().to_path_buf();
    seed(
        &path,
        "task_id,task_name,status\nT1,First,not_started\nT2,Second,not_started\n",
    )
    .unwrap();
    app.reload_tasks().unwrap();
    app.tasks.set_view(crate::tasks::view::View::All);
    app.execute_command(Command::Task(TaskCommand::MarkComplete));
    key(&mut app, KeyCode::Enter);
    app.run_remove("T2");
    wait_preview(&mut app);
    let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(app.context.workspace()).unwrap();
    let start = Instant::now();
    key(&mut app, KeyCode::Char('d'));
    assert!(start.elapsed() < Duration::from_millis(200));
    assert!(app.overlay.is_none());
    assert!(
        app.tasks
            .completion_identity("T2", app.context.workspace().id())
            .is_none()
    );
    app.reload_tasks().unwrap();
    app.tasks.cycle_view_next();
    assert!(
        app.tasks
            .completion_identity("T2", app.context.workspace().id())
            .is_none()
    );
    app.execute_command(Command::Global(
        crate::tui::action::GlobalAction::ShowShortcuts,
    ));
    drop(owner);
    drain(&mut app);
    assert!(
        app.overlay.is_some(),
        "result must not dismiss a newer modal"
    );
    assert!(!std::fs::read_to_string(&path).unwrap().contains("T2"));
    assert!(std::fs::read_to_string(&path).unwrap().contains("done"));
}

#[test]
fn confirmation_uuid_cannot_drift_after_refresh_reassigns_display_id() {
    let temp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temp, &cli, AgentKind::Claude);
    let path = app.context.tasks_csv_path().to_path_buf();
    let original = crate::tasks::identity::TaskUuid::new();
    let other = crate::tasks::identity::TaskUuid::new();
    seed(
        &path,
        format!("task_uuid,task_id,task_name,status\n{original},T1,Original,not_started\n"),
    )
    .unwrap();
    app.reload_tasks().unwrap();
    app.tasks.set_view(crate::tasks::view::View::All);
    app.execute_command(Command::Task(TaskCommand::MarkComplete));
    seed(&path,format!("task_uuid,task_id,task_name,status\n{other},T1,Other,not_started\n{original},T9,Original,not_started\n")).unwrap();
    app.reload_tasks().unwrap();
    key(&mut app, KeyCode::Enter);
    drain(&mut app);
    let rows = crate::tasks::task::load_tasks(&path).unwrap();
    assert!(!rows[0].is_done());
    assert!(rows[1].is_done());
}

fn seed(path: &std::path::Path, text: impl AsRef<str>) -> std::io::Result<()> {
    let mut csv = crate::tasks::complete::parse_csv_bytes(text.as_ref().as_bytes()).unwrap();
    for column in "task_type,priority,due_date,hard_deadline,start_date,see_also,notes,project,energy_level,context,estimated_duration,blocked_by,defer_count,created_date,completed_date,last_touched,linear_issue".split(',') {
        if !csv.header.iter().any(|v|v==column) { csv.header.push(column.into()); }
    }
    std::fs::write(path, crate::tasks::complete::serialize_csv(&csv).unwrap())
}

#[test]
fn pending_changes_have_persistent_visible_status() {
    let temp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temp, &cli, AgentKind::Claude);
    seed(
        app.context.tasks_csv_path(),
        "task_id,task_name,status\nT1,First,not_started\n",
    )
    .unwrap();
    app.reload_tasks().unwrap();
    app.tasks.set_view(crate::tasks::view::View::All);
    let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(app.context.workspace()).unwrap();
    app.execute_command(Command::Task(TaskCommand::MarkComplete));
    key(&mut app, KeyCode::Enter);
    app.status.clear_flash();
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| crate::tui::draw::draw(frame, &mut app))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(text.contains("Saving 1 task change"), "{text}");
    drop(owner);
    drain(&mut app);
}

#[test]
fn completion_commit_keeps_done_state_and_reports_agenda_warning() {
    let temp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temp, &cli, AgentKind::Claude);
    seed(
        app.context.tasks_csv_path(),
        "task_id,task_name,status\nT1,First,not_started\n",
    )
    .unwrap();
    let directory = temp.path().join("agenda");
    let date = chrono::Local::now().date_naive();
    std::fs::create_dir_all(directory.join(format!("{date}.md"))).unwrap();
    crate::env::set(
        app.context.command(),
        "agenda_markdown_dir",
        directory.to_str().unwrap(),
    )
    .unwrap();
    app.reload_tasks().unwrap();
    app.tasks.set_view(crate::tasks::view::View::All);
    app.execute_command(Command::Task(TaskCommand::MarkComplete));
    key(&mut app, KeyCode::Enter);
    drain(&mut app);
    assert!(
        app.tasks
            .completion_identity("T1", app.context.workspace().id())
            .unwrap()
            .1
    );
    assert!(
        app.status
            .error()
            .is_some_and(|error| error.contains("agenda"))
    );
}

#[test]
fn duplicate_confirmation_does_not_enqueue_same_occurrence_twice() {
    let temp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temp, &cli, AgentKind::Claude);
    seed(
        app.context.tasks_csv_path(),
        "task_id,task_name,status\nT1,First,not_started\n",
    )
    .unwrap();
    app.reload_tasks().unwrap();
    let identity = app
        .tasks
        .completion_identity("T1", app.context.workspace().id())
        .unwrap()
        .0;
    let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(app.context.workspace()).unwrap();
    app.queue_completion(identity.clone(), chrono::Local::now().date_naive());
    app.queue_completion(identity, chrono::Local::now().date_naive());
    let pending = app.services.pending_task_changes();
    drop(owner);
    drain(&mut app);
    assert_eq!(pending, 1);
}

#[test]
fn every_quit_route_waits_for_accepted_jobs_but_preview_alone_can_exit() {
    let temp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temp, &cli, AgentKind::Claude);
    seed(
        app.context.tasks_csv_path(),
        "task_id,task_name,status\nT1,First,not_started\n",
    )
    .unwrap();
    app.reload_tasks().unwrap();
    app.tasks.set_view(crate::tasks::view::View::All);
    let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(app.context.workspace()).unwrap();
    app.run_remove("T1");
    assert!(crate::tui::event_loop::update_application(
        &mut app,
        &Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL))
    ));
    crate::tui::overlay::close_overlay(&mut app.overlay);
    let identity = app
        .tasks
        .completion_identity("T1", app.context.workspace().id())
        .unwrap()
        .0;
    app.queue_completion(identity, chrono::Local::now().date_naive());
    app.shell.focus_tasks();
    for (code, modifiers) in [
        (KeyCode::Char('q'), KeyModifiers::NONE),
        (KeyCode::Esc, KeyModifiers::NONE),
        (KeyCode::Char('c'), KeyModifiers::CONTROL),
        (KeyCode::Char('q'), KeyModifiers::CONTROL),
    ] {
        assert!(!crate::tui::event_loop::update_application(
            &mut app,
            &Event::Key(KeyEvent::new(code, modifiers))
        ));
    }
    app.show_logs_view(crate::tui::logs_view::LogKind::Brain);
    assert!(!key(&mut app, KeyCode::Char('q')));
    app.execute_command(Command::Global(crate::tui::action::GlobalAction::Quit));
    assert!(!key(&mut app, KeyCode::Null));
    drop(owner);
    drain(&mut app);
    assert!(key(&mut app, KeyCode::Char('q')));
}

#[test]
fn failed_habit_refresh_keeps_the_last_authoritative_rows() {
    let temp = tempfile::tempdir().unwrap();
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temp, &cli, AgentKind::Claude);
    seed(
        app.context.tasks_csv_path(),
        "task_id,task_name,status\nT1,First,not_started\n",
    )
    .unwrap();
    app.reload_tasks().unwrap();
    let path = app.context.tasks_csv_path().with_file_name("habits.csv");
    std::fs::write(path, "task_id\nH1\n").unwrap();
    assert!(app.reload_tasks().is_err());
    assert!(
        app.tasks
            .completion_identity("T1", app.context.workspace().id())
            .is_some()
    );
}
