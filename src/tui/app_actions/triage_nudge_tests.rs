use super::{Task, triage_nudge_target};

fn d(y: i32, m: u32, day: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

/// Builds a Morning Triage occurrence with a given id/due/completed state.
/// Self-contained literal (rather than reusing `task::test_task`, which is
/// `#[cfg(test)]`-gated to that module) so this test owns its fixtures.
fn triage(id: &str, due: chrono::NaiveDate, completed: Option<chrono::NaiveDate>) -> Task {
    Task {
        task_uuid: None,
        id: id.to_owned(),
        name: "Morning Triage (5mins)".to_owned(),
        types: Vec::new(),
        status: if completed.is_some() {
            "done"
        } else {
            "not_started"
        }
        .to_owned(),
        priority: "p1".to_owned(),
        due_date: Some(due),
        hard_deadline: false,
        start_date: None,
        assigned_to: String::new(),
        notes: String::new(),
        project: String::new(),
        energy: String::new(),
        context: String::new(),
        estimated_duration: None,
        defer_count: 0,
        last_touched: None,
        see_also: String::new(),
        blocked_by: Vec::new(),
        completed_date: completed,
        linear_issue: String::new(),
        system_key: String::new(),
    }
}

#[test]
fn fires_when_no_occurrence_completed_today() {
    let today = d(2026, 6, 24);
    // Yesterday's occurrence is done; today's is still open.
    let habits = vec![
        triage("H31", d(2026, 6, 23), Some(d(2026, 6, 23))),
        triage("H41", d(2026, 6, 24), None),
    ];
    let target = triage_nudge_target(&habits, "Morning Triage", today);
    assert_eq!(target.map(|h| h.id.as_str()), Some("H41"));
}

#[test]
fn silent_when_todays_occurrence_completed() {
    let today = d(2026, 6, 24);
    // This is the regression case: today's occurrence (H41) is done,
    // even though a *different* id (H31) was yesterday's. A name match
    // sees the completion; an old fixed-ID check on H31 would not.
    let habits = vec![
        triage("H31", d(2026, 6, 23), Some(d(2026, 6, 23))),
        triage("H41", d(2026, 6, 24), Some(d(2026, 6, 24))),
        triage("H47", d(2026, 6, 25), None),
    ];
    assert!(triage_nudge_target(&habits, "Morning Triage", today).is_none());
}

#[test]
fn case_insensitive_and_tolerates_suffix() {
    let today = d(2026, 6, 24);
    let habits = vec![triage("H41", d(2026, 6, 24), None)];
    // Lowercase pattern still matches "Morning Triage (5mins)".
    assert!(triage_nudge_target(&habits, "morning triage", today).is_some());
}

#[test]
fn empty_pattern_disables_check() {
    let today = d(2026, 6, 24);
    let habits = vec![triage("H41", d(2026, 6, 24), None)];
    assert!(triage_nudge_target(&habits, "  ", today).is_none());
}

#[test]
fn invalid_regex_is_silent() {
    let today = d(2026, 6, 24);
    let habits = vec![triage("H41", d(2026, 6, 24), None)];
    // Unbalanced bracket — must not panic, must not fire.
    assert!(triage_nudge_target(&habits, "Morning [Triage", today).is_none());
}

#[test]
fn no_match_is_silent() {
    let today = d(2026, 6, 24);
    let habits = vec![triage("H41", d(2026, 6, 24), None)];
    assert!(triage_nudge_target(&habits, "Weekly Review", today).is_none());
}

#[test]
fn disabled_flag_suppresses_the_nudge() {
    use super::triage_modal_target;
    let today = d(2026, 6, 24);
    // An open occurrence due today would normally fire the modal.
    let habits = vec![triage("H41", d(2026, 6, 24), None)];
    assert!(triage_modal_target(true, false, &habits, "Morning Triage", today).is_some());
    // The process-scoped opt-out (config-seeded, palette-flipped) suppresses it.
    assert!(triage_modal_target(true, true, &habits, "Morning Triage", today).is_none());
    // The portable feature flag wins over every process preference.
    assert!(triage_modal_target(false, false, &habits, "Morning Triage", today).is_none());
}

#[test]
fn surfaces_due_today_over_other_matches() {
    let today = d(2026, 6, 24);
    // Out-of-order vec: ensure we pick the due-today row, not the first.
    let habits = vec![
        triage("H47", d(2026, 6, 25), None),
        triage("H41", d(2026, 6, 24), None),
        triage("H31", d(2026, 6, 23), None),
    ];
    let target = triage_nudge_target(&habits, "Morning Triage", today);
    assert_eq!(target.map(|h| h.id.as_str()), Some("H41"));
}

#[test]
fn an_outstanding_triage_with_no_nudge_up_raises_one() {
    use super::{TriageAlertOccupancy, TriageAlertResolution, resolve_triage_alert};

    assert_eq!(
        resolve_triage_alert(true, TriageAlertOccupancy::Empty),
        TriageAlertResolution::Open
    );
}

#[test]
fn a_nudge_the_sync_proved_stale_is_withdrawn() {
    use super::{TriageAlertOccupancy, TriageAlertResolution, resolve_triage_alert};

    // Triage was completed on another machine while the modal was on screen.
    // Leaving it up invites the user to answer a question already answered — and
    // re-run a pass that already ran.
    assert_eq!(
        resolve_triage_alert(false, TriageAlertOccupancy::TriageNudge),
        TriageAlertResolution::Dismiss
    );
}

#[test]
fn an_already_correct_screen_is_left_alone() {
    use super::{TriageAlertOccupancy, TriageAlertResolution, resolve_triage_alert};

    // Outstanding and already asking: leave the user's modal untouched rather
    // than rebuilding it under them.
    assert_eq!(
        resolve_triage_alert(true, TriageAlertOccupancy::TriageNudge),
        TriageAlertResolution::Leave
    );
    // Nothing outstanding and nothing showing.
    assert_eq!(
        resolve_triage_alert(false, TriageAlertOccupancy::Empty),
        TriageAlertResolution::Leave
    );
}

#[test]
fn startup_sync_under_help_waits_for_dismissal_then_displays_triage() {
    use super::{
        TriageAlertOccupancy, TriageAlertResolution, resolve_triage_alert,
        triage_reconciliation_pending,
    };

    let while_help_is_open = resolve_triage_alert(true, TriageAlertOccupancy::OtherOverlay);
    assert_eq!(while_help_is_open, TriageAlertResolution::Defer);
    assert!(
        triage_reconciliation_pending(while_help_is_open),
        "sync completion must remain pending while Help owns the overlay slot"
    );

    let after_help_is_dismissed = resolve_triage_alert(true, TriageAlertOccupancy::Empty);
    assert_eq!(after_help_is_dismissed, TriageAlertResolution::Open);
    assert!(
        !triage_reconciliation_pending(after_help_is_dismissed),
        "opening the triage nudge completes the deferred startup decision"
    );
}
