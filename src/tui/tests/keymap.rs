//! Tests for the pure key classifiers: count prefix, view shortcuts, search
//! delegation, and the app-level chords.

use crate::tasks::view::View;
use crate::tui::*;
use crossterm::event::{KeyCode, KeyModifiers};

// --- accumulate_count (vim-style count prefix) ---

#[test]
fn first_digit_starts_a_count() {
    assert_eq!(accumulate_count(None, 3), Some(3));
}

#[test]
fn subsequent_digits_shift_and_append() {
    // Typing `1` then `2` then `0` builds 120 (e.g. 120j).
    let c = accumulate_count(None, 1);
    let c = accumulate_count(c, 2);
    assert_eq!(accumulate_count(c, 0), Some(120));
}

#[test]
fn leading_zero_is_not_a_count() {
    // A bare `0` doesn't start a count, leaving the key free.
    assert_eq!(accumulate_count(None, 0), None);
}

#[test]
fn zero_extends_an_in_progress_count() {
    // But `0` after a non-zero digit is a normal digit: `1` then `0`.
    assert_eq!(accumulate_count(Some(1), 0), Some(10));
}

#[test]
fn count_is_capped_at_max() {
    // Runaway digit entry saturates rather than overflowing.
    let mut c = Some(MAX_COUNT);
    c = accumulate_count(c, 9);
    assert_eq!(c, Some(MAX_COUNT));
}

#[test]
fn digits_and_motions_preserve_a_pending_count() {
    for c in ['0', '5', '9', 'j', 'k'] {
        assert!(is_count_relevant_key(KeyCode::Char(c), false));
    }
    assert!(is_count_relevant_key(KeyCode::Up, false));
    assert!(is_count_relevant_key(KeyCode::Down, false));
}

#[test]
fn other_keys_clear_a_pending_count() {
    // Non-motion normal keys, and any ctrl-modified key, are not
    // count-relevant — they clear the prefix.
    assert!(!is_count_relevant_key(KeyCode::Char('g'), false));
    assert!(!is_count_relevant_key(KeyCode::Char('d'), false));
    assert!(!is_count_relevant_key(KeyCode::Char('l'), false));
    assert!(!is_count_relevant_key(KeyCode::Enter, false));
    assert!(!is_count_relevant_key(KeyCode::Char('/'), false));
    // Ctrl chords never participate, even on otherwise-relevant keys.
    assert!(!is_count_relevant_key(KeyCode::Char('j'), true));
    assert!(!is_count_relevant_key(KeyCode::Char('5'), true));
}

// --- h_collapses_notes ---

#[test]
fn h_collapses_when_highlighted_entry_has_expanded_notes() {
    // The motivating case: notes are expanded on the highlighted
    // entry, so `h` must collapse them rather than jump to habits.
    assert!(h_collapses_notes(true, true));
}

#[test]
fn h_switches_to_habits_when_notes_collapsed() {
    // Entry has notes but they're collapsed — `h` is the habits
    // shortcut as usual.
    assert!(!h_collapses_notes(true, false));
}

#[test]
fn h_switches_to_habits_when_entry_has_no_notes() {
    // No notes to collapse (even if `full_notes` reports "expanded"),
    // so `h` stays the habits shortcut and never dead-ends.
    assert!(!h_collapses_notes(false, true));
    assert!(!h_collapses_notes(false, false));
}

// --- search_delegates_ctrl_chord ---

#[test]
fn ctrl_enter_delegates_to_normal_in_search_mode() {
    // The motivating case: Ctrl+Enter opens the task actions modal on
    // the highlighted task even while the user is typing in the search
    // input (bare Enter exits search, so it can't do double duty).
    assert!(search_delegates_ctrl_chord(KeyCode::Enter, true));
}

#[test]
fn ctrl_d_delegates_to_normal_in_search_mode() {
    // Ctrl+D ("done") must mark-complete the highlighted task from
    // inside `/` without first exiting search.
    assert!(search_delegates_ctrl_chord(KeyCode::Char('d'), true));
}

#[test]
fn bare_enter_stays_in_search_mode() {
    // Enter without ctrl is search-specific: exit-search-keep-filter.
    assert!(!search_delegates_ctrl_chord(KeyCode::Enter, false));
}

#[test]
fn ctrl_c_keeps_search_specific_handling() {
    // Ctrl+C is handled search-specifically (it exits `/` instead of
    // quitting the shell), so it must not be bounced to normal-mode.
    assert!(!search_delegates_ctrl_chord(KeyCode::Char('c'), true));
}

// --- search_key_abandons_filter ---

#[test]
fn ctrl_c_in_search_abandons_filter_not_quits() {
    // The motivating bug: Ctrl+C while typing in `/` should leave search
    // mode (clearing the filter), exactly like Esc — never quit the shell.
    assert!(search_key_abandons_filter(KeyCode::Char('c'), true));
}

#[test]
fn esc_in_search_abandons_filter() {
    // Esc has always exited `/` and cleared the filter.
    assert!(search_key_abandons_filter(KeyCode::Esc, false));
}

#[test]
fn bare_c_in_search_is_text_input() {
    // A bare `c` is query text, not an abandon-search chord.
    assert!(!search_key_abandons_filter(KeyCode::Char('c'), false));
}

#[test]
fn ctrl_u_in_search_does_not_abandon_filter() {
    // Ctrl+U clears the query but stays in search mode — it is not an
    // abandon-and-exit chord.
    assert!(!search_key_abandons_filter(KeyCode::Char('u'), true));
}

#[test]
fn ctrl_u_keeps_search_specific_handling() {
    // Ctrl+U clears the query in search mode (readline-style). It
    // must NOT fall through to normal-mode's bare-`u` half-page-up
    // navigation.
    assert!(!search_delegates_ctrl_chord(KeyCode::Char('u'), true));
}

// --- search_edit_key_exits_when_empty ---

#[test]
fn ctrl_u_exits_search_when_query_empty() {
    // On an empty query, Ctrl+U has nothing to clear, so it doubles as
    // an exit — the same "press again to leave" behavior as Backspace.
    assert!(search_edit_key_exits_when_empty(KeyCode::Char('u'), true));
}

#[test]
fn backspace_exits_search_when_query_empty() {
    // The pre-existing behavior Ctrl+U now mirrors.
    assert!(search_edit_key_exits_when_empty(KeyCode::Backspace, false));
}

#[test]
fn bare_u_does_not_exit_empty_search() {
    // Without ctrl, `u` is query text — it never exits search.
    assert!(!search_edit_key_exits_when_empty(KeyCode::Char('u'), false));
}

#[test]
fn bare_letter_does_not_exit_empty_search() {
    assert!(!search_edit_key_exits_when_empty(KeyCode::Char('t'), false));
}

#[test]
fn ctrl_letter_chords_delegate() {
    // Ctrl-modified chords (e.g. Ctrl+D → mark complete, Ctrl+Enter →
    // task actions modal) fall through to normal-mode handling when
    // typed inside `/`.
    assert!(search_delegates_ctrl_chord(KeyCode::Char('r'), true));
    assert!(search_delegates_ctrl_chord(KeyCode::Enter, true));
}

#[test]
fn bare_letter_stays_in_search_mode() {
    // Without ctrl, letters are text input for the query — never
    // a normal-mode shortcut.
    assert!(!search_delegates_ctrl_chord(KeyCode::Char('t'), false));
    assert!(!search_delegates_ctrl_chord(KeyCode::Char('a'), false));
}

// --- view_shortcut ---

#[test]
fn view_shortcut_bare_letters_map_to_views() {
    assert_eq!(view_shortcut(KeyCode::Char('t'), false), Some(View::Today));
    assert_eq!(view_shortcut(KeyCode::Char('m'), false), Some(View::Mit));
    assert_eq!(
        view_shortcut(KeyCode::Char('p'), false),
        Some(View::PastDue)
    );
    assert_eq!(view_shortcut(KeyCode::Char('w'), false), Some(View::Week));
    assert_eq!(view_shortcut(KeyCode::Char('a'), false), Some(View::All));
}

#[test]
fn view_shortcut_ctrl_modified_never_switches_views() {
    // Ctrl+<letter> must not switch views — otherwise Ctrl+P would
    // collide with the command-palette chord.
    for c in ['t', 'm', 'p', 'w', 'a'] {
        assert_eq!(view_shortcut(KeyCode::Char(c), true), None);
    }
}

#[test]
fn view_shortcut_ignores_unrelated_keys() {
    assert_eq!(view_shortcut(KeyCode::Char('z'), false), None);
    assert_eq!(view_shortcut(KeyCode::Enter, false), None);
}

// --- ctrl_opens_palette ---

#[test]
fn ctrl_p_opens_the_palette() {
    assert!(ctrl_opens_palette(KeyCode::Char('p')));
    assert!(ctrl_opens_palette(KeyCode::Char('P')));
}

#[test]
fn ctrl_k_no_longer_opens_the_palette() {
    assert!(!ctrl_opens_palette(KeyCode::Char('k')));
    assert!(!ctrl_opens_palette(KeyCode::Char('t')));
}

// --- ctrl_quits ---

#[test]
fn ctrl_q_quits_the_shell() {
    assert!(ctrl_quits(KeyCode::Char('q'), true));
    assert!(ctrl_quits(KeyCode::Char('Q'), true));
}

#[test]
fn bare_q_is_not_the_global_quit_chord() {
    // Bare `q` quits too, but via the normal-mode handler (tasks panel
    // only). The global chord requires Ctrl so it also reaches us from
    // the brain panel, where bare `q` is forwarded to claude.
    assert!(!ctrl_quits(KeyCode::Char('q'), false));
    assert!(!ctrl_quits(KeyCode::Char('Q'), false));
}

#[test]
fn ctrl_other_keys_do_not_quit() {
    assert!(!ctrl_quits(KeyCode::Char('c'), true));
    assert!(!ctrl_quits(KeyCode::Char('x'), true));
}

// --- alt_scroll_direction ---

#[test]
fn alt_u_and_alt_d_scroll_by_focused_panel() {
    assert_eq!(
        alt_scroll_direction(KeyCode::Char('u'), KeyModifiers::ALT),
        Some(true)
    );
    assert_eq!(
        alt_scroll_direction(KeyCode::Char('D'), KeyModifiers::ALT | KeyModifiers::SHIFT),
        Some(false)
    );
}

#[test]
fn option_generated_macos_glyphs_scroll_too() {
    assert_eq!(
        alt_scroll_direction(KeyCode::Char('\u{00a8}'), KeyModifiers::NONE),
        Some(true)
    );
    assert_eq!(
        alt_scroll_direction(KeyCode::Char('\u{2202}'), KeyModifiers::NONE),
        Some(false)
    );
}

#[test]
fn non_scroll_alt_keys_are_ignored() {
    assert_eq!(
        alt_scroll_direction(KeyCode::Char('s'), KeyModifiers::ALT),
        None
    );
    assert_eq!(
        alt_scroll_direction(KeyCode::Enter, KeyModifiers::ALT),
        None
    );
}

// --- alt_selects_brain_tab ---

#[test]
fn alt_1_and_alt_2_select_brain_tabs() {
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('1'), KeyModifiers::ALT),
        Some(BrainTab::Main)
    );
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('2'), KeyModifiers::ALT),
        Some(BrainTab::Triage)
    );
}

#[test]
fn option_generated_macos_digit_glyphs_select_tabs() {
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('\u{00a1}'), KeyModifiers::NONE),
        Some(BrainTab::Main)
    );
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('\u{2122}'), KeyModifiers::NONE),
        Some(BrainTab::Triage)
    );
}

#[test]
fn a_bare_digit_never_selects_a_tab() {
    // The whole reason for Alt over Ctrl: a plain `1`/`2` must stay ordinary
    // input (it types into the brain PTY), never a tab switch.
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('1'), KeyModifiers::NONE),
        None
    );
    assert_eq!(
        alt_selects_brain_tab(KeyCode::Char('3'), KeyModifiers::ALT),
        None
    );
}

// --- ctrl_opens_links ---

#[test]
fn ctrl_o_opens_links() {
    assert!(ctrl_opens_links(KeyCode::Char('o'), true));
    assert!(ctrl_opens_links(KeyCode::Char('O'), true));
}

#[test]
fn bare_o_does_not_open_links() {
    // Without ctrl, `o` is an ordinary key — never the open action.
    assert!(!ctrl_opens_links(KeyCode::Char('o'), false));
    assert!(!ctrl_opens_links(KeyCode::Char('O'), false));
}

#[test]
fn ctrl_l_no_longer_opens_links() {
    // The binding moved off Ctrl+L; bare `l` (notes toggle) and Ctrl+L
    // must not trigger the open action.
    assert!(!ctrl_opens_links(KeyCode::Char('l'), true));
    assert!(!ctrl_opens_links(KeyCode::Char('k'), true));
    assert!(!ctrl_opens_links(KeyCode::Char('d'), true));
}

// --- ctrl_removes_task ---

#[test]
fn ctrl_backspace_removes_task() {
    assert!(ctrl_removes_task(KeyCode::Backspace, true));
}

#[test]
fn bare_backspace_does_not_remove_task() {
    // Plain Backspace is a no-op in the task list: it's too easy to hit
    // by accident, so removal requires the Ctrl modifier.
    assert!(!ctrl_removes_task(KeyCode::Backspace, false));
}

#[test]
fn ctrl_other_keys_do_not_remove_task() {
    assert!(!ctrl_removes_task(KeyCode::Char('d'), true));
    assert!(!ctrl_removes_task(KeyCode::Delete, true));
}
