//! Raw input forwarding: mouse-wheel routing between the two panels and
//! keystroke forwarding into the brain PTY's stdin.

use crate::tui::*;

use crossterm::event::KeyCode;

/// Number of tasks a single wheel notch moves the selection in the tasks
/// panel, and the rows it scrolls the brain panel's history. Kept modest so
/// trackpad inertia (many events) stays controllable.
const WHEEL_TASKS: usize = 1;
const WHEEL_ROWS: usize = 3;

#[must_use]
pub(crate) const fn brain_key_starts_turn(code: KeyCode) -> bool {
    matches!(code, KeyCode::Enter)
}

/// How many rows a single Alt+U / Alt+D press moves: half the focused pane's
/// visible rows, never less than one so a tiny (or zero-height) pane still
/// advances instead of freezing.
pub(crate) fn half_page_step(visible_rows: u16) -> usize {
    (usize::from(visible_rows) / 2).max(1)
}

/// Route a mouse wheel event to whichever panel the cursor is over. The
/// tasks panel moves its selection (mirroring j/k); the brain panel scrolls
/// its vt100 scrollback. Modals are captive, so wheel events are ignored
/// while any overlay is open.
pub(crate) fn handle_mouse(app: &mut App<'_>, me: crossterm::event::MouseEvent) {
    use crossterm::event::MouseEventKind;

    // True modal overlays are captive — swallow the wheel entirely.
    if app.palette.is_some()
        || app.brain_input.is_some()
        || app.confirm.is_some()
        || app.help.is_some()
    {
        return;
    }

    let up = match me.kind {
        MouseEventKind::ScrollUp => true,
        MouseEventKind::ScrollDown => false,
        _ => return,
    };

    match panel_at(app.brain_rect, me.column, me.row) {
        Panel::Brain => {
            if let Some(controller) = app.active_brain_controller_mut() {
                if up {
                    let _ = controller.scroll_up(WHEEL_ROWS);
                } else {
                    let _ = controller.scroll_down(WHEEL_ROWS);
                }
            }
        }
        Panel::Tasks => {
            if up {
                app.select_prev(WHEEL_TASKS);
            } else {
                app.select_next(WHEEL_TASKS);
            }
        }
    }
}

/// Forward a keystroke into the PTY's stdin. Alt+H / Alt+L are handled
/// upstream in `event_loop` and never reach here. When the child has
/// exited, a Ctrl-C / q / Esc closes the panel instead of being forwarded
/// (there's no process to receive it).
pub(crate) fn handle_brain_key(
    app: &mut App<'_>,
    k: &crossterm::event::KeyEvent,
    ctrl: bool,
) -> bool {
    let mut alive = app
        .brain
        .as_ref()
        .is_some_and(|controller| controller.is_alive().unwrap_or(false));
    if !alive {
        // Child gone: close the panel on Ctrl-C / Esc / q so the user can
        // get back to a full-width tasks view without re-spawning.
        match k.code {
            KeyCode::Char('c') if ctrl => {
                app.close_brain();
                return false;
            }
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => {
                app.close_brain();
                return false;
            }
            _ => return false,
        }
    }

    // A remote message is mid-answer: the panel is the sender's conversation
    // and every keystroke would land in the composer beside the injected
    // prompt, so only the interrupt key gets through.
    if !crate::tui::receiver_state::forwards_local_keystroke(
        app.receiver_started.is_some(),
        ctrl && matches!(k.code, KeyCode::Char('c' | 'C')),
    ) {
        app.flash = Some(FlashKind::Info(
            "Brain is answering a received message — input is locked until it replies (Ctrl+C interrupts)".to_owned(),
        ));
        return false;
    }

    let Some(bytes) = key_to_bytes(k) else {
        return false;
    };
    app.leave_warm_receiver_for_interactive_input();
    alive = app
        .brain
        .as_ref()
        .is_some_and(|controller| controller.is_alive().unwrap_or(false));
    if !alive {
        return false;
    }
    let starts_turn = brain_key_starts_turn(k.code);
    if let Some(controller) = app.brain.as_mut() {
        // Typing snaps back to the live tail so the prompt is always in
        // view, even if the user had scrolled up through history.
        let _ = controller.scroll_to_bottom();
        let result = if starts_turn {
            controller.submit_now()
        } else {
            controller.forward_terminal_input(bytes)
        };
        if let Err(error) = result {
            crate::logging::log(format!("brain input failed: {error}"));
        } else if starts_turn {
            app.mark_brain_turn_started();
        }
    }
    false
}

/// Forward a keystroke into the active skill-session PTY. A skill-session tab is
/// deliberately outside the receiver/turn machinery (it is untracked and
/// self-closing), so this is a plain forwarder: encode the key and write it.
/// When that session has exited, a Ctrl-C / Esc / q closes the tab so the user
/// can get back to the main session.
pub(crate) fn handle_skill_session_key(
    app: &mut App<'_>,
    k: &crossterm::event::KeyEvent,
    ctrl: bool,
) -> bool {
    let alive = app
        .active_brain_controller()
        .is_some_and(|controller| controller.is_alive().unwrap_or(false));
    if !alive {
        match k.code {
            KeyCode::Char('c') if ctrl => app.close_active_skill_session(),
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => app.close_active_skill_session(),
            _ => {}
        }
        return false;
    }
    if let Some(bytes) = key_to_bytes(k) {
        if let Some(controller) = app.active_brain_controller_mut() {
            let _ = controller.scroll_to_bottom();
            let result = if brain_key_starts_turn(k.code) {
                controller.submit_now()
            } else {
                controller.forward_terminal_input(bytes)
            };
            if let Err(error) = result {
                crate::logging::log(format!("skill session input failed: {error}"));
            }
        }
    }
    false
}
