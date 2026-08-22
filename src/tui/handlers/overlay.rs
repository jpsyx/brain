//! Key handlers for the captive overlay modals: command palette, confirm,
//! link-picker, brain-input, and help. Each is a no-op unless its modal is
//! open; while open the modal is captive (the event loop routes straight here).

use crossterm::event::{KeyCode, KeyModifiers};

use crate::tui::App;
use crate::tui::keymap::enter_inserts_newline;
use crate::tui::modal_state::{ConfirmChoice, ConfirmKind, ConfirmState};
use crate::tui::overlay::{Overlay, close_overlay};
use crate::tui::palette::PaletteStep;

pub(crate) fn handle_palette_key(app: &mut App, k: &crossterm::event::KeyEvent, _ctrl: bool) {
    let Some(Overlay::TaskPalette(palette)) = app.overlay.as_mut() else {
        return;
    };
    match palette.handle_key(*k) {
        PaletteStep::Continue => {}
        PaletteStep::Cancel => {
            close_overlay(&mut app.overlay);
        }
        PaletteStep::Confirm(action) => app.execute_task_action(action),
    }
}

/// Take the active confirm modal and dispatch its Yes path to the right
/// action handler. No-op when no confirm is open.
pub(crate) fn run_confirm_yes(app: &mut App) {
    let Some(c) = take_task_confirmation(app) else {
        return;
    };
    match c.kind {
        ConfirmKind::MarkComplete => app.run_mark_complete(&c.task_id),
        ConfirmKind::Remove => app.run_remove(&c.task_id),
        ConfirmKind::GenerateAgenda => app.run_generate_agenda(),
        ConfirmKind::RunTriage => app.run_triage(),
    }
}

/// Take the active confirm modal and run its Skip path. Only the
/// daily-triage modal offers Skip; other kinds have no Skip button, so
/// this is a no-op for them (defensive — the key handler already gates
/// `s` on `has_skip`).
pub(crate) fn run_confirm_skip(app: &mut App) {
    let Some(c) = take_task_confirmation(app) else {
        return;
    };
    if c.kind == ConfirmKind::RunTriage {
        app.skip_triage();
    }
}

/// Resolve the confirm modal by the button the user landed on: `Yes` runs
/// the action, `No` cancels, `Skip` runs the skip path.
pub(crate) fn run_confirm_choice(app: &mut App, choice: ConfirmChoice) {
    match choice {
        ConfirmChoice::Yes => run_confirm_yes(app),
        ConfirmChoice::No => {
            close_overlay(&mut app.overlay);
        }
        ConfirmChoice::Skip => run_confirm_skip(app),
    }
}

pub(crate) fn handle_confirm_key(app: &mut App, k: &crossterm::event::KeyEvent, ctrl: bool) {
    if !matches!(app.overlay, Some(Overlay::TaskConfirmation(_))) {
        return;
    }
    match k.code {
        // Cancel paths: Esc / Ctrl-C / N / explicit No-then-Enter (the
        // "Enter with No focused" case is handled below).
        KeyCode::Esc | KeyCode::Char('n' | 'N') => {
            close_overlay(&mut app.overlay);
        }
        KeyCode::Char('c') if ctrl => {
            close_overlay(&mut app.overlay);
        }
        // Y immediately confirms regardless of which button is focused.
        KeyCode::Char('y' | 'Y') => run_confirm_yes(app),
        // S immediately skips, but only on modals that offer a Skip button
        // (the daily-triage nudge). Ignored elsewhere.
        KeyCode::Char('s' | 'S') if matches!(app.overlay.as_ref(), Some(Overlay::TaskConfirmation(confirm)) if confirm.has_skip()) =>
        {
            run_confirm_skip(app);
        }
        // Enter resolves with the currently-focused button.
        KeyCode::Enter => {
            if let Some(choice) = app.overlay.as_ref().and_then(|overlay| match overlay {
                Overlay::TaskConfirmation(confirm) => Some(confirm.focus),
                Overlay::TaskPalette(_)
                | Overlay::BrainInput(_)
                | Overlay::SearchPalette(_)
                | Overlay::SearchConfirmation(_)
                | Overlay::LinkPicker(_)
                | Overlay::AssigneeFilter(_)
                | Overlay::Help(_)
                | Overlay::SyncLog(_) => None,
            }) {
                run_confirm_choice(app, choice);
            }
        }
        // Button focus movement. Left group steps toward Yes; right group
        // steps toward No / Skip. Clamped at the ends.
        KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
            if let Some(Overlay::TaskConfirmation(confirm)) = app.overlay.as_mut() {
                confirm.focus_prev();
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
            if let Some(Overlay::TaskConfirmation(confirm)) = app.overlay.as_mut() {
                confirm.focus_next();
            }
        }
        _ => {}
    }
}

/// Handle a keystroke for the link-picker modal. ↑/↓ (and Ctrl+J / Ctrl+K)
/// move the selection; a digit jumps to and opens that numbered row; Enter
/// opens the highlighted link; Esc / Ctrl+C dismiss. Other keys are
/// swallowed — the modal is captive until resolved.
pub(crate) fn handle_link_picker_key(app: &mut App, k: &crossterm::event::KeyEvent, ctrl: bool) {
    let Some(Overlay::LinkPicker(picker)) = app.overlay.as_mut() else {
        return;
    };
    match k.code {
        KeyCode::Esc => {
            close_overlay(&mut app.overlay);
        }
        KeyCode::Char('c') if ctrl => {
            close_overlay(&mut app.overlay);
        }
        KeyCode::Up => picker.move_up(),
        KeyCode::Down => picker.move_down(),
        KeyCode::Char('k' | 'K') if ctrl => picker.move_up(),
        KeyCode::Char('j' | 'J') if ctrl => picker.move_down(),
        // A bare digit is a direct jump-and-open on that numbered row,
        // mirroring the brain menu's numbered selection.
        KeyCode::Char(c) if !ctrl && c.is_ascii_digit() => {
            if let Some(n) = c.to_digit(10) {
                if picker.select_number(n as usize) {
                    app.open_selected_link();
                }
            }
        }
        KeyCode::Enter => app.open_selected_link(),
        _ => {}
    }
}

/// Handle the shared-workspace assignee filter. The numbered rows mirror the
/// link picker, but choosing a row changes the in-process task filter instead
/// of opening a URL.
pub(crate) fn handle_assignee_filter_key(
    app: &mut App,
    k: &crossterm::event::KeyEvent,
    ctrl: bool,
) {
    let Some(Overlay::AssigneeFilter(picker)) = app.overlay.as_mut() else {
        return;
    };
    let apply = match k.code {
        KeyCode::Esc => {
            close_overlay(&mut app.overlay);
            return;
        }
        KeyCode::Char('c') if ctrl => {
            close_overlay(&mut app.overlay);
            return;
        }
        KeyCode::Up => {
            picker.move_up();
            false
        }
        KeyCode::Down => {
            picker.move_down();
            false
        }
        KeyCode::Char('k' | 'K') if ctrl => {
            picker.move_up();
            false
        }
        KeyCode::Char('j' | 'J') if ctrl => {
            picker.move_down();
            false
        }
        KeyCode::Char(c) if !ctrl && c.is_ascii_digit() => c
            .to_digit(10)
            .is_some_and(|number| picker.select_number(number as usize)),
        KeyCode::Enter => true,
        _ => false,
    };
    if apply {
        let selected = picker.selected_user();
        close_overlay(&mut app.overlay);
        app.tasks.set_assignment_filter(selected);
    }
}

pub(crate) fn handle_brain_input_key(app: &mut App, k: &crossterm::event::KeyEvent, ctrl: bool) {
    if !matches!(app.overlay, Some(Overlay::BrainInput(_))) {
        return;
    }
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    match k.code {
        KeyCode::Esc => {
            close_overlay(&mut app.overlay);
        }
        KeyCode::Char('c') if ctrl => {
            close_overlay(&mut app.overlay);
        }
        // Alt+Enter inserts a newline instead of sending, so the user can
        // compose a multiline message. See `enter_inserts_newline` for why
        // Alt+Enter rather than Shift+Enter.
        KeyCode::Enter if enter_inserts_newline(alt) => {
            if let Some(Overlay::BrainInput(state)) = app.overlay.as_mut() {
                state.buffer.push('\n');
            }
        }
        KeyCode::Enter => {
            // Take ownership so `finalize` can move the buffer + context.
            let Some(Overlay::BrainInput(state)) = close_overlay(&mut app.overlay) else {
                return;
            };
            if let Some(message) = state.finalize() {
                // Open / focus the persistent brain panel and seed it with the
                // composed message (task context prefix already applied).
                app.send_brain_prompt(&message);
            }
        }
        KeyCode::Char('u') if ctrl => {
            if let Some(Overlay::BrainInput(state)) = app.overlay.as_mut() {
                state.buffer.clear();
            }
        }
        KeyCode::Backspace => {
            if let Some(Overlay::BrainInput(state)) = app.overlay.as_mut() {
                state.buffer.pop();
            }
        }
        KeyCode::Char(c) if !ctrl => {
            if let Some(Overlay::BrainInput(state)) = app.overlay.as_mut() {
                state.buffer.push(c);
            }
        }
        _ => {}
    }
}

/// Handle a keystroke for the keyboard-shortcuts help modal. `j`/`k`/arrows
/// scroll the (possibly long) list; `g`/`G` jump to the ends; `?` / `q` /
/// `Esc` / `Ctrl+C` dismiss it. Captive while open.
pub(crate) fn handle_help_key(app: &mut App, k: &crossterm::event::KeyEvent, ctrl: bool) {
    let Some(Overlay::Help(help)) = app.overlay.as_mut() else {
        return;
    };
    match k.code {
        KeyCode::Char('q' | 'Q' | '?') | KeyCode::Esc => {
            close_overlay(&mut app.overlay);
        }
        KeyCode::Char('c') if ctrl => {
            close_overlay(&mut app.overlay);
        }
        KeyCode::Char('j') | KeyCode::Down => help.scroll = help.scroll.saturating_add(1),
        KeyCode::Char('k') | KeyCode::Up => help.scroll = help.scroll.saturating_sub(1),
        KeyCode::Char('g') | KeyCode::Home => help.scroll = 0,
        KeyCode::PageDown => help.scroll = help.scroll.saturating_add(10),
        KeyCode::PageUp => help.scroll = help.scroll.saturating_sub(10),
        _ => {}
    }
}

/// Keys for the live sync-log modal.
///
/// The modal follows the tail by default (`scroll: u16::MAX` is clamped to the
/// last page when drawn), so `k` steps back through history and `G` returns to
/// following.
pub(crate) fn handle_sync_log_key(app: &mut App, k: &crossterm::event::KeyEvent) {
    let Some(Overlay::SyncLog(log)) = app.overlay.as_mut() else {
        return;
    };
    match k.code {
        KeyCode::Char('q' | 'Q') | KeyCode::Esc => {
            close_overlay(&mut app.overlay);
        }
        KeyCode::Char('j') | KeyCode::Down => log.scroll = log.scroll.saturating_add(1),
        KeyCode::Char('k') | KeyCode::Up => log.scroll = log.scroll.saturating_sub(1),
        KeyCode::Char('g') | KeyCode::Home => log.scroll = 0,
        KeyCode::Char('G') | KeyCode::End => log.scroll = u16::MAX,
        KeyCode::PageDown => log.scroll = log.scroll.saturating_add(10),
        KeyCode::PageUp => log.scroll = log.scroll.saturating_sub(10),
        _ => {}
    }
}

fn take_task_confirmation(app: &mut App) -> Option<ConfirmState> {
    match close_overlay(&mut app.overlay) {
        Some(Overlay::TaskConfirmation(confirm)) => Some(confirm),
        other => {
            app.overlay = other;
            None
        }
    }
}
