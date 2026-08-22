use super::*;
use crate::tui::{HelpState, Overlay, close_overlay};

fn arm_completed_refresh_gate(app: &mut App) {
    app.status
        .arm_triage_gate(Some(4), std::time::Instant::now());
    app.status.mark_triage_refresh_complete();
}

fn triage_habit(today: chrono::NaiveDate, completed: bool) -> crate::tasks::task::Task {
    crate::tasks::task::Task {
        task_uuid: None,
        id: "H1".to_owned(),
        name: "Morning Triage".to_owned(),
        types: Vec::new(),
        status: if completed { "done" } else { "not_started" }.to_owned(),
        priority: "p2".to_owned(),
        due_date: Some(today),
        hard_deadline: false,
        start_date: None,
        assigned_to: "pablo".to_owned(),
        notes: String::new(),
        project: String::new(),
        energy: String::new(),
        context: String::new(),
        estimated_duration: None,
        defer_count: 0,
        last_touched: None,
        see_also: String::new(),
        blocked_by: Vec::new(),
        completed_date: completed.then_some(today),
        linear_issue: String::new(),
        system_key: "brain.triage.daily".to_owned(),
    }
}

#[test]
fn completed_startup_sync_waits_for_help_to_close_before_showing_triage() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let today = app.tasks.daily_triage_date();
    app.tasks
        .replace_rows(Vec::new(), vec![triage_habit(today, false)]);
    let mut config = app.context.config().clone();
    config.enable_triage_habits = true;
    app.context = app.context.replacing_config(config);
    app.status.set_daily_triage_check_disabled(false);
    arm_completed_refresh_gate(&mut app);
    app.overlay = Some(Overlay::Help(HelpState { scroll: 0 }));

    app.tick_triage_gate();

    assert!(matches!(app.overlay, Some(Overlay::Help(_))));
    assert!(
        app.status.triage_gate_is_armed(),
        "the refreshed triage decision must survive while Help is active"
    );

    close_overlay(&mut app.overlay);
    app.tick_triage_gate();

    assert!(matches!(
        app.overlay,
        Some(Overlay::TaskConfirmation(ref confirmation))
            if confirmation.kind == crate::tui::ConfirmKind::RunTriage
    ));
    assert!(
        !app.status.triage_gate_is_armed(),
        "displaying the triage nudge completes the startup gate"
    );
}

#[test]
fn completed_startup_sync_still_withdraws_an_open_stale_triage_nudge() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let cli = Cli::parse_from(["tasks"]);
    let mut app = test_app(&temporary, &cli, AgentKind::Claude);
    let today = app.tasks.daily_triage_date();
    app.tasks
        .replace_rows(Vec::new(), vec![triage_habit(today, true)]);
    let mut config = app.context.config().clone();
    config.enable_triage_habits = true;
    app.context = app.context.replacing_config(config);
    app.status.set_daily_triage_check_disabled(false);
    arm_completed_refresh_gate(&mut app);
    app.overlay = Some(Overlay::TaskConfirmation(
        crate::tui::ConfirmState::run_triage("H1".to_owned(), "Morning Triage".to_owned()),
    ));

    app.tick_triage_gate();

    assert!(app.overlay.is_none(), "the stale triage nudge must close");
    assert!(
        !app.status.triage_gate_is_armed(),
        "reconciliation completes the gate"
    );
}
