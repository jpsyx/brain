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

    assert_eq!(
        resolve_triage_alert(false, TriageAlertOccupancy::TriageNudge),
        TriageAlertResolution::Dismiss
    );
}

#[test]
fn an_already_correct_screen_is_left_alone() {
    use super::{TriageAlertOccupancy, TriageAlertResolution, resolve_triage_alert};

    assert_eq!(
        resolve_triage_alert(true, TriageAlertOccupancy::TriageNudge),
        TriageAlertResolution::Leave
    );
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
    assert!(triage_reconciliation_pending(while_help_is_open));

    let after_help_is_dismissed = resolve_triage_alert(true, TriageAlertOccupancy::Empty);
    assert_eq!(after_help_is_dismissed, TriageAlertResolution::Open);
    assert!(!triage_reconciliation_pending(after_help_is_dismissed));
}
