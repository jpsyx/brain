//! Key handlers for the captive overlay modals: command palette, confirm,
//! link-picker, brain-input, and help. Each is a no-op unless its modal is
//! open; while open the modal is captive (the event loop routes straight here).

use crate::tui::*;

use crossterm::event::{KeyCode, KeyModifiers};

pub(crate) fn handle_palette_key(app: &mut App<'_>, k: &crossterm::event::KeyEvent, ctrl: bool) {
    let Some(palette) = app.palette.as_mut() else {
        return;
    };
    match k.code {
        KeyCode::Esc => app.palette = None,
        KeyCode::Char('c') if ctrl => app.palette = None,
        KeyCode::Enter => {
            if let Some(action) = palette.selected_action() {
                app.execute_palette_action(action);
            }
        }
        KeyCode::Up => palette.move_up(),
        KeyCode::Down => palette.move_down(),
        // Ctrl+J / Ctrl+K aliases for ↓ / ↑ — vim-flavored navigation
        // for users with hands already on the home row. Distinct from
        // bare j / k (which are typed into the filter as letters).
        // Requires kitty-protocol disambiguation in the host terminal,
        // same caveat as Ctrl+M.
        KeyCode::Char('j' | 'J') if ctrl => palette.move_down(),
        KeyCode::Char('k' | 'K') if ctrl => palette.move_up(),
        KeyCode::Backspace => palette.pop(),
        KeyCode::Char(c) if !ctrl => palette.append(c),
        _ => {}
    }
}

/// Take the active confirm modal and dispatch its Yes path to the right
/// action handler. No-op when no confirm is open.
pub(crate) fn run_confirm_yes(app: &mut App<'_>) {
    let Some(c) = app.confirm.take() else { return };
    match c.kind {
        ConfirmKind::MarkComplete => app.run_mark_complete(&c.task_id),
        ConfirmKind::Remove => app.run_remove(&c.task_id),
        ConfirmKind::GenerateAgenda => app.run_generate_agenda(),
        ConfirmKind::RunTriage => app.run_triage(),
        ConfirmKind::ShowLogs => {
            if let Some(path) = c.path.as_deref() {
                app.run_show_logs(path);
            }
        }
    }
}

/// Take the active confirm modal and run its Skip path. Only the
/// daily-triage modal offers Skip; other kinds have no Skip button, so
/// this is a no-op for them (defensive — the key handler already gates
/// `s` on `has_skip`).
pub(crate) fn run_confirm_skip(app: &mut App<'_>) {
    let Some(c) = app.confirm.take() else { return };
    if c.kind == ConfirmKind::RunTriage {
        app.skip_triage();
    }
}

/// Resolve the confirm modal by the button the user landed on: `Yes` runs
/// the action, `No` cancels, `Skip` runs the skip path.
pub(crate) fn run_confirm_choice(app: &mut App<'_>, choice: ConfirmChoice) {
    match choice {
        ConfirmChoice::Yes => run_confirm_yes(app),
        ConfirmChoice::No => app.confirm = None,
        ConfirmChoice::Skip => run_confirm_skip(app),
    }
}

pub(crate) fn handle_confirm_key(app: &mut App<'_>, k: &crossterm::event::KeyEvent, ctrl: bool) {
    if app.confirm.is_none() {
        return;
    }
    match k.code {
        // Cancel paths: Esc / Ctrl-C / N / explicit No-then-Enter (the
        // "Enter with No focused" case is handled below).
        KeyCode::Esc | KeyCode::Char('n' | 'N') => app.confirm = None,
        KeyCode::Char('c') if ctrl => app.confirm = None,
        // Y immediately confirms regardless of which button is focused.
        KeyCode::Char('y' | 'Y') => run_confirm_yes(app),
        // S immediately skips, but only on modals that offer a Skip button
        // (the daily-triage nudge). Ignored elsewhere.
        KeyCode::Char('s' | 'S') if app.confirm.as_ref().is_some_and(ConfirmState::has_skip) => {
            run_confirm_skip(app);
        }
        // Enter resolves with the currently-focused button.
        KeyCode::Enter => {
            if let Some(choice) = app.confirm.as_ref().map(|c| c.focus) {
                run_confirm_choice(app, choice);
            }
        }
        // Button focus movement. Left group steps toward Yes; right group
        // steps toward No / Skip. Clamped at the ends.
        KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
            if let Some(c) = app.confirm.as_mut() {
                c.focus_prev();
            }
        }
        KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
            if let Some(c) = app.confirm.as_mut() {
                c.focus_next();
            }
        }
        _ => {}
    }
}

/// Handle a keystroke for the link-picker modal. ↑/↓ (and Ctrl+J / Ctrl+K)
/// move the selection; a digit jumps to and opens that numbered row; Enter
/// opens the highlighted link; Esc / Ctrl+C dismiss. Other keys are
/// swallowed — the modal is captive until resolved.
pub(crate) fn handle_link_picker_key(
    app: &mut App<'_>,
    k: &crossterm::event::KeyEvent,
    ctrl: bool,
) {
    let Some(picker) = app.link_picker.as_mut() else {
        return;
    };
    match k.code {
        KeyCode::Esc => app.link_picker = None,
        KeyCode::Char('c') if ctrl => app.link_picker = None,
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

pub(crate) fn handle_brain_input_key(
    app: &mut App<'_>,
    k: &crossterm::event::KeyEvent,
    ctrl: bool,
) {
    if app.brain_input.is_none() {
        return;
    }
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    match k.code {
        KeyCode::Esc => app.brain_input = None,
        KeyCode::Char('c') if ctrl => app.brain_input = None,
        // Alt+Enter inserts a newline instead of sending, so the user can
        // compose a multiline message. See `enter_inserts_newline` for why
        // Alt+Enter rather than Shift+Enter.
        KeyCode::Enter if enter_inserts_newline(alt) => {
            if let Some(state) = app.brain_input.as_mut() {
                state.buffer.push('\n');
            }
        }
        KeyCode::Enter => {
            // Take ownership so `finalize` can move the buffer + context.
            let Some(state) = app.brain_input.take() else {
                return;
            };
            if let Some(message) = state.finalize() {
                // Open / focus the persistent brain panel and seed it with the
                // composed message (task context prefix already applied).
                app.send_brain_prompt(&message);
            }
        }
        KeyCode::Char('u') if ctrl => {
            if let Some(state) = app.brain_input.as_mut() {
                state.buffer.clear();
            }
        }
        KeyCode::Backspace => {
            if let Some(state) = app.brain_input.as_mut() {
                state.buffer.pop();
            }
        }
        KeyCode::Char(c) if !ctrl => {
            if let Some(state) = app.brain_input.as_mut() {
                state.buffer.push(c);
            }
        }
        _ => {}
    }
}

/// Handle a keystroke for the keyboard-shortcuts help modal. `j`/`k`/arrows
/// scroll the (possibly long) list; `g`/`G` jump to the ends; `?` / `q` /
/// `Esc` / `Ctrl+C` dismiss it. Captive while open.
pub(crate) fn handle_help_key(app: &mut App<'_>, k: &crossterm::event::KeyEvent, ctrl: bool) {
    let Some(help) = app.help.as_mut() else {
        return;
    };
    match k.code {
        KeyCode::Char('q' | 'Q' | '?') | KeyCode::Esc => app.help = None,
        KeyCode::Char('c') if ctrl => app.help = None,
        KeyCode::Char('j') | KeyCode::Down => help.scroll = help.scroll.saturating_add(1),
        KeyCode::Char('k') | KeyCode::Up => help.scroll = help.scroll.saturating_sub(1),
        KeyCode::Char('g') | KeyCode::Home => help.scroll = 0,
        KeyCode::PageDown => help.scroll = help.scroll.saturating_add(10),
        KeyCode::PageUp => help.scroll = help.scroll.saturating_sub(10),
        _ => {}
    }
}
