//! Tests for the brain-open / message-about-task Ctrl+M classifiers and
//! the Alt+Enter newline rule.

use crate::tui::*;
use crossterm::event::KeyCode;

// --- enter_inserts_newline: multiline compose in the brain-input modal ---

#[test]
fn bare_enter_submits_not_newline() {
    assert!(!enter_inserts_newline(false));
}

#[test]
fn alt_enter_inserts_a_newline() {
    // Alt+Enter is the reliable newline binding (distinct Meta sequence
    // on every terminal); a bare Enter still submits.
    assert!(enter_inserts_newline(true));
}

// --- ctrl_opens_brain vs ctrl_messages_brain_about_task: Shift is what
//     splits Ctrl+M (panel) from Ctrl+Shift+M (task-scoped message) ---

#[test]
fn ctrl_m_without_shift_opens_brain() {
    assert!(ctrl_opens_brain(KeyCode::Char('m'), true, false));
    // Kitty may report the shifted glyph; the chord still resolves.
    assert!(ctrl_opens_brain(KeyCode::Char('M'), true, false));
}

#[test]
fn ctrl_shift_m_does_not_open_brain() {
    // Shift held → that's the task-scoped message, not the panel toggle.
    assert!(!ctrl_opens_brain(KeyCode::Char('m'), true, true));
    assert!(!ctrl_opens_brain(KeyCode::Char('M'), true, true));
}

#[test]
fn bare_m_does_not_open_brain() {
    // No Ctrl → that's the "jump to MIT view" letter, not a brain chord.
    assert!(!ctrl_opens_brain(KeyCode::Char('m'), false, false));
}

#[test]
fn ctrl_shift_m_messages_brain_about_task() {
    assert!(ctrl_messages_brain_about_task(KeyCode::Char('m'), true, true));
    assert!(ctrl_messages_brain_about_task(KeyCode::Char('M'), true, true));
}

#[test]
fn ctrl_m_without_shift_does_not_message_about_task() {
    assert!(!ctrl_messages_brain_about_task(KeyCode::Char('m'), true, false));
}

#[test]
fn shift_m_without_ctrl_does_not_message_about_task() {
    // Plain Shift+M (a capital M keystroke) is not a chord.
    assert!(!ctrl_messages_brain_about_task(KeyCode::Char('M'), false, true));
}
