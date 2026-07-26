//! Tests for shell_quote, BrainInputState::finalize, the ConfirmState
//! constructors / choices / intents, mouse hit-testing, and the submit countdown.

use crate::session::shell_quote;
use crate::tui::*;

// --- shell_quote ---

#[test]
fn shell_quote_wraps_plain_string_in_single_quotes() {
    assert_eq!(shell_quote("hello"), "'hello'");
}

#[test]
fn shell_quote_empty_string_is_two_quotes() {
    assert_eq!(shell_quote(""), "''");
}

#[test]
fn shell_quote_escapes_embedded_single_quote() {
    // POSIX trick: close, escape, reopen.
    assert_eq!(shell_quote("it's"), "'it'\\''s'");
}

#[test]
fn shell_quote_escapes_multiple_single_quotes() {
    assert_eq!(shell_quote("'a'b'"), "''\\''a'\\''b'\\'''");
}

// --- BrainInputState::finalize ---

#[test]
fn finalize_empty_buffer_returns_none() {
    let s = BrainInputState::about("T1".to_owned(), "x".to_owned());
    assert!(s.finalize().is_none());
}

#[test]
fn finalize_whitespace_only_buffer_returns_none() {
    let mut s = BrainInputState::about("T1".to_owned(), "x".to_owned());
    s.buffer = "   \t  ".to_owned();
    assert!(s.finalize().is_none());
}

#[test]
fn finalize_trims_the_buffer_inside_the_context_prefix() {
    let mut s = BrainInputState::about("T1".to_owned(), "x".to_owned());
    s.buffer = "  hi there  ".to_owned();
    assert_eq!(
        s.finalize().unwrap(),
        "This message is about T1 (x): hi there"
    );
}

#[test]
fn finalize_with_task_context_includes_id_and_label() {
    let mut s = BrainInputState::about("T123".to_owned(), "Fix login".to_owned());
    s.buffer = "what's the latest?".to_owned();
    assert_eq!(
        s.finalize().unwrap(),
        "This message is about T123 (Fix login): what's the latest?"
    );
}

// --- ConfirmState constructors ---

#[test]
fn generate_agenda_ctor_uses_correct_kind_and_title() {
    let s = ConfirmState::generate_agenda();
    assert_eq!(s.kind, ConfirmKind::GenerateAgenda);
    assert!(s.title.contains("agenda"));
    assert_eq!(s.focus, ConfirmChoice::Yes, "Yes should be default-focused");
}

#[test]
fn run_triage_ctor_carries_task_id_and_label() {
    let s = ConfirmState::run_triage("H31".to_owned(), "Morning Triage".to_owned());
    assert_eq!(s.kind, ConfirmKind::RunTriage);
    assert_eq!(s.task_id, "H31");
    assert_eq!(s.task_label, "Morning Triage");
}

#[test]
fn show_logs_confirm_carries_the_full_log_path() {
    let s = ConfirmState::show_logs(std::path::PathBuf::from("/tmp/2026.log"));
    assert_eq!(s.kind, ConfirmKind::ShowLogs);
    assert_eq!(
        s.path.as_deref(),
        Some(std::path::Path::new("/tmp/2026.log"))
    );
    assert!(s.prompt.contains("/tmp/2026.log"), "{}", s.prompt);
    assert_eq!(s.choices(), &[ConfirmChoice::Yes, ConfirmChoice::No]);
}

// --- ConfirmChoice: the triage modal alone offers a third "Skip" button ---

#[test]
fn triage_confirm_offers_yes_no_skip() {
    let s = ConfirmState::run_triage("H31".to_owned(), "Morning Triage".to_owned());
    assert_eq!(
        s.choices(),
        &[ConfirmChoice::Yes, ConfirmChoice::No, ConfirmChoice::Skip]
    );
    assert!(s.has_skip());
}

#[test]
fn non_triage_confirms_are_yes_no_only() {
    for s in [
        ConfirmState::mark_complete("T1".to_owned(), "x".to_owned()),
        ConfirmState::remove("T1".to_owned(), "x".to_owned()),
        ConfirmState::generate_agenda(),
        ConfirmState::show_logs(std::path::PathBuf::from("/tmp/x.log")),
    ] {
        assert_eq!(s.choices(), &[ConfirmChoice::Yes, ConfirmChoice::No]);
        assert!(!s.has_skip());
    }
}

#[test]
fn confirm_focus_defaults_to_yes() {
    assert_eq!(
        ConfirmState::run_triage("H1".to_owned(), "T".to_owned()).focus,
        ConfirmChoice::Yes
    );
    assert_eq!(
        ConfirmState::mark_complete("T1".to_owned(), "x".to_owned()).focus,
        ConfirmChoice::Yes
    );
}

#[test]
fn triage_focus_walks_all_three_and_clamps_at_the_ends() {
    let mut s = ConfirmState::run_triage("H1".to_owned(), "T".to_owned());
    s.focus_next();
    assert_eq!(s.focus, ConfirmChoice::No);
    s.focus_next();
    assert_eq!(s.focus, ConfirmChoice::Skip);
    s.focus_next();
    assert_eq!(s.focus, ConfirmChoice::Skip, "clamps at the right end");
    s.focus_prev();
    assert_eq!(s.focus, ConfirmChoice::No);
    s.focus_prev();
    assert_eq!(s.focus, ConfirmChoice::Yes);
    s.focus_prev();
    assert_eq!(s.focus, ConfirmChoice::Yes, "clamps at the left end");
}

#[test]
fn binary_confirm_focus_never_reaches_skip() {
    let mut s = ConfirmState::mark_complete("T1".to_owned(), "x".to_owned());
    s.focus_next();
    assert_eq!(s.focus, ConfirmChoice::No);
    s.focus_next();
    assert_eq!(s.focus, ConfirmChoice::No, "no Skip button to move onto");
}

#[test]
fn skip_triage_prompt_uses_the_documented_skip_language() {
    // The brain agent recognizes "skip daily triage" (the /triage +
    // /todo skills' skip trigger) and marks the Morning Triage habit
    // done rather than running a pass. Keep the phrase intact.
    let p = SKIP_TRIAGE_PROMPT.to_lowercase();
    assert!(
        p.contains("skip daily triage"),
        "prompt was: {SKIP_TRIAGE_PROMPT}"
    );
}

// --- ConfirmIntent: green for constructive, red for destructive ---

#[test]
fn mark_complete_is_a_success_intent() {
    // Completing a task is constructive, so the modal reads green.
    let s = ConfirmState::mark_complete("T1".to_owned(), "x".to_owned());
    assert_eq!(s.intent, ConfirmIntent::Success);
}

#[test]
fn remove_is_a_danger_intent() {
    let s = ConfirmState::remove("T1".to_owned(), "x".to_owned());
    assert_eq!(s.intent, ConfirmIntent::Danger);
}

#[test]
fn agenda_and_triage_are_success_intents() {
    assert_eq!(
        ConfirmState::generate_agenda().intent,
        ConfirmIntent::Success
    );
    assert_eq!(
        ConfirmState::run_triage("H1".to_owned(), "Triage".to_owned()).intent,
        ConfirmIntent::Success
    );
}

#[test]
fn intent_accents_differ_success_green_danger_red() {
    // The two intents must map to distinct accents (green vs red).
    assert_ne!(
        ConfirmIntent::Success.accent(),
        ConfirmIntent::Danger.accent()
    );
    // Sanity: green channel dominates for Success, red for Danger.
    assert_eq!(ConfirmIntent::Success.accent(), Color::Rgb(158, 206, 106));
    assert_eq!(ConfirmIntent::Danger.accent(), Color::Rgb(247, 118, 142));
}

// --- mouse-scroll panel hit-testing ---

#[test]
fn panel_at_returns_tasks_when_no_brain_panel() {
    // Full-width tasks: every coordinate routes to the tasks panel.
    assert_eq!(panel_at(None, 0, 0), Panel::Tasks);
    assert_eq!(panel_at(None, 79, 23), Panel::Tasks);
}

#[test]
fn panel_at_splits_on_the_brain_rect() {
    use ratatui::layout::Rect;
    // 80-col split: tasks on the left, brain occupying x=40..80.
    let brain = Some(Rect {
        x: 40,
        y: 0,
        width: 40,
        height: 24,
    });
    // A column inside the brain rect → Brain.
    assert_eq!(panel_at(brain, 50, 10), Panel::Brain);
    assert_eq!(panel_at(brain, 40, 0), Panel::Brain);
    // A column left of the brain rect → Tasks.
    assert_eq!(panel_at(brain, 39, 10), Panel::Tasks);
    assert_eq!(panel_at(brain, 0, 0), Panel::Tasks);
}

// --- Alt+U / Alt+D half-page scroll step ---

#[test]
fn half_page_step_is_half_the_visible_rows() {
    // A 40-row pane scrolls 20 rows per Alt+U/Alt+D.
    assert_eq!(half_page_step(40), 20);
    assert_eq!(half_page_step(41), 20);
}

#[test]
fn half_page_step_never_falls_below_one_on_tiny_panes() {
    // A 0- or 1-row pane must still advance by a full row, never freeze.
    assert_eq!(half_page_step(0), 1);
    assert_eq!(half_page_step(1), 1);
}

// --- deferred brain-submit countdown ---

#[test]
fn submit_countdown_is_quiet_when_nothing_is_pending() {
    assert_eq!(advance_submit_countdown(0), (0, false));
}

#[test]
fn submit_countdown_fires_the_return_exactly_once() {
    // A two-tick delay: the first tick just decrements…
    let (after_first, fire_first) = advance_submit_countdown(2);
    assert_eq!((after_first, fire_first), (1, false));
    // …the second tick lands at zero and fires the submitting Return…
    let (after_second, fire_second) = advance_submit_countdown(after_first);
    assert_eq!((after_second, fire_second), (0, true));
    // …and once at zero it stays quiet, so the Enter is sent only once.
    assert_eq!(advance_submit_countdown(after_second), (0, false));
}
