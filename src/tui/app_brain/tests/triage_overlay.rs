use super::*;
use crate::tui::{HelpState, Overlay, TriageGate, close_overlay};

fn completed_refresh_gate() -> TriageGate {
    TriageGate {
        seen_journal_id: Some(4),
        next_poll: std::time::Instant::now(),
        refresh_complete: true,
    }
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
    app.config.enable_triage_habits = true;
    app.skip_daily_triage_check = false;
    app.triage_gate = Some(completed_refresh_gate());
    app.overlay = Some(Overlay::Help(HelpState { scroll: 0 }));

    app.tick_triage_gate();

    assert!(matches!(app.overlay, Some(Overlay::Help(_))));
    assert!(
        app.triage_gate.is_some(),
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
        app.triage_gate.is_none(),
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
    app.config.enable_triage_habits = true;
    app.skip_daily_triage_check = false;
    app.triage_gate = Some(completed_refresh_gate());
    app.overlay = Some(Overlay::TaskConfirmation(
        crate::tui::ConfirmState::run_triage("H1".to_owned(), "Morning Triage".to_owned()),
    ));

    app.tick_triage_gate();

    assert!(app.overlay.is_none(), "the stale triage nudge must close");
    assert!(
        app.triage_gate.is_none(),
        "reconciliation completes the gate"
    );
}
