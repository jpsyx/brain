//! Per-surface key handlers (palette / confirm / brain-input / normal /
//! completion / brain / search).

use super::*;

use crossterm::event::{
        KeyCode, KeyModifiers,
    };
use crate::pty_pane::PtyPane;
use crate::tasks::view::View;

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

pub(crate) fn handle_brain_input_key(app: &mut App<'_>, k: &crossterm::event::KeyEvent, ctrl: bool) {
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

/// Returns `true` when the loop should exit. Navigation operates on the
/// selected task rather than on raw lines: j / k / ↓ / ↑ move one task,
/// d / u jump half a screen of tasks, PgDn / PgUp move a full
/// screen, and g / G jump to the first / last task. Scroll follows the
/// selection automatically via `ensure_selected_visible`.
pub(crate) fn handle_normal_key(app: &mut App<'_>, code: KeyCode, ctrl: bool) -> bool {
    // Vim-style count prefix: bare digits accumulate into `pending_count`
    // and wait for a motion key. Every other keystroke consumes (and thus
    // clears) the pending count below via `.take()`.
    if !ctrl {
        if let KeyCode::Char(c) = code {
            if let Some(d) = c.to_digit(10) {
                if let Some(n) = accumulate_count(app.pending_count, d) {
                    app.pending_count = Some(n);
                    return false;
                }
            }
        }
    }
    let count = app.pending_count.take().unwrap_or(1);

    // Single-letter view shortcuts (bare letters only). `view_shortcut`
    // returns `None` for ctrl-modified keys, so `Ctrl+P` (palette) and the
    // other chords never collide with the bare view letters. `h` is handled
    // in the match below because it doubles as a notes-collapse key.
    if let Some(view) = view_shortcut(code, ctrl) {
        app.set_view(view);
        return false;
    }

    // Ctrl+O opens the selected task's links (Linear issue + notes URLs):
    // one link opens directly, several raise the picker modal. No-op when
    // the task has no openable link.
    if ctrl_opens_links(code, ctrl) {
        app.run_open_links();
        return false;
    }

    match code {
        KeyCode::Char('q' | 'Q') => return true,
        KeyCode::Char('c') if ctrl => return true,
        KeyCode::Esc => {
            if app.has_active_filter() {
                app.clear_query();
            } else {
                return true;
            }
        }
        KeyCode::Char('/') => app.in_search = true,
        KeyCode::Tab => app.cycle_view_next(),
        KeyCode::BackTab => app.cycle_view_prev(),

        KeyCode::Char('h') if !ctrl => {
            // Sole exception to "`h` = habits view": when the highlighted
            // entry's notes are expanded, `h` collapses them instead.
            if h_collapses_notes(app.current_has_notes(), app.current_notes_expanded()) {
                app.toggle_notes();
            } else {
                app.set_view(View::Habits);
            }
        }

        // Re-read the CSVs from disk and repaint. Useful when tasks
        // have been completed or deferred from outside the tasks shell (or via
        // the brain panel in a way that didn't trigger an auto-reload).
        KeyCode::Char('r') => app.refresh(),

        // (Help moved to the app-level `Alt+S` accelerator, handled in
        // `event_loop`; bare `?` is no longer a help binding.)

        // Ctrl+D ("done") on a selected task is a direct mark-complete
        // shortcut (with a Yes/No confirmation, since this mutates
        // tasks.csv). Unlike Ctrl+Enter, Ctrl+D is 0x04 and has no Enter
        // aliasing, so it works on every terminal regardless of the kitty
        // keyboard protocol.
        KeyCode::Char('d') if ctrl => {
            // Clone fields before mutating self.confirm to drop the
            // shared borrow on visible_tasks first.
            let target = app
                .selected_task
                .and_then(|i| app.visible_tasks.get(i))
                .map(|t| (t.id.clone(), t.name.clone()));
            if let Some((id, label)) = target {
                app.confirm = Some(ConfirmState::mark_complete(id, label));
            }
        }

        // Ctrl+Backspace on a highlighted task is a destructive shortcut for
        // the Remove action — opens the confirmation modal first, then
        // the Yes path calls `run_remove`. Bare Backspace is intentionally a
        // no-op (too easy to fat-finger into a deletion). Skipped for habits
        // since their removal path is different (handled elsewhere via the
        // brain agent's habit-specific flow).
        KeyCode::Backspace if ctrl_removes_task(code, ctrl) => {
            if app.current_is_habit() {
                return false;
            }
            let target = app
                .selected_task
                .and_then(|i| app.visible_tasks.get(i))
                .map(|t| (t.id.clone(), t.name.clone()));
            if let Some((id, label)) = target {
                app.confirm = Some(ConfirmState::remove(id, label));
            }
        }

        // Enter on a selected entry opens a focused palette of only
        // task-scoped actions. Habit-incompatible commands are filtered
        // out automatically. No-op when nothing is selected. Ctrl+Enter
        // routes here too (no `if ctrl` arm precedes it): in search mode
        // bare Enter exits `/`, so Ctrl+Enter is how the user opens the
        // actions modal without leaving the search input.
        KeyCode::Enter => {
            // Clone (id, name) up front so the shared borrow on
            // visible_tasks ends before we mutate app.palette.
            let target = app
                .selected_task
                .and_then(|i| app.visible_tasks.get(i))
                .map(|t| (t.id.clone(), t.name.clone()));
            if let Some((id, label)) = target {
                let is_habit = app.current_is_habit();
                let has_notes = app.current_has_notes();
                let notes_expanded = app.current_notes_expanded();
                let link_kind = app.current_link_kind();
                app.palette = Some(PaletteState::new_task_actions(
                    id,
                    label,
                    is_habit,
                    has_notes,
                    notes_expanded,
                    link_kind,
                ));
            }
        }

        // One task at a time, or `count` tasks when a vim-style numeric
        // prefix preceded the motion (e.g. `3j` moves down 3).
        KeyCode::Down | KeyCode::Char('j') => app.select_next(count),
        KeyCode::Up | KeyCode::Char('k') => app.select_prev(count),

        // Page-step navigation. `d`/`u` ~ half a screen; PgDn pages down,
        // PgUp pages up. (Bare `b` is the backlog-view jump, handled by
        // `view_shortcut` above; the letter aliases `f`/`b` were retired in
        // favor of the dedicated page keys.)
        KeyCode::Char('d') => {
            let step = (app.tasks_per_page() / 2).max(1);
            app.select_next(step);
        }
        KeyCode::Char('u') => {
            let step = (app.tasks_per_page() / 2).max(1);
            app.select_prev(step);
        }
        KeyCode::PageDown => {
            app.select_next(app.tasks_per_page().max(1));
        }
        KeyCode::PageUp => {
            app.select_prev(app.tasks_per_page().max(1));
        }
        KeyCode::Home | KeyCode::Char('g') => app.select_first(),
        KeyCode::End | KeyCode::Char('G') => app.select_last(),

        // Toggle the selected task's notes between a single-line preview and
        // the full markdown-rendered body. Inert when the task has no notes.
        KeyCode::Char('l') => app.toggle_notes(),

        // Arrow aliases for notes on a task that has them: → expands,
        // ← collapses. No-op otherwise.
        KeyCode::Right => app.expand_notes(),
        KeyCode::Left => app.collapse_notes(),

        _ => {}
    }
    false
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

/// Number of tasks a single wheel notch moves the selection in the tasks
/// panel, and the rows it scrolls the brain panel's history. Kept modest so
/// trackpad inertia (many events) stays controllable.
const WHEEL_TASKS: usize = 1;
const WHEEL_ROWS: usize = 3;

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
            if let Some(pty) = app.brain.as_ref() {
                if up {
                    pty.scroll_up(WHEEL_ROWS);
                } else {
                    pty.scroll_down(WHEEL_ROWS);
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
pub(crate) fn handle_brain_key(app: &mut App<'_>, k: &crossterm::event::KeyEvent, ctrl: bool) -> bool {
    let alive = app.brain.as_ref().is_some_and(PtyPane::is_alive);
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

    let Some(bytes) = key_to_bytes(k) else {
        return false;
    };
    if let Some(pty) = app.brain.as_ref() {
        // Typing snaps back to the live tail so the prompt is always in
        // view, even if the user had scrolled up through history.
        pty.scroll_to_bottom();
        pty.send(bytes);
    }
    false
}
/// Returns `true` when the loop should exit.
pub(crate) fn handle_search_key(app: &mut App<'_>, code: KeyCode, ctrl: bool) -> bool {
    if search_delegates_ctrl_chord(code, ctrl) {
        return handle_normal_key(app, code, ctrl);
    }
    // Esc and Ctrl+C both back out of `/`: leave search mode and drop the
    // filter. Ctrl+C used to quit the whole shell here — it now mirrors Esc.
    if search_key_abandons_filter(code, ctrl) {
        app.in_search = false;
        app.clear_query();
        return false;
    }
    // Backspace and Ctrl+U edit the query, but on an empty query they back
    // out of search instead (a second Ctrl+U after clearing leaves `/`).
    if search_edit_key_exits_when_empty(code, ctrl) && app.query.is_empty() {
        app.in_search = false;
        return false;
    }
    match code {
        KeyCode::Enter => app.in_search = false,
        KeyCode::Backspace => app.pop_query(),
        KeyCode::Char('u') if ctrl => app.clear_query(),
        KeyCode::Char(c) if !ctrl => app.append_query(c),

        // Arrow keys move the selection (j / k are typed into the query
        // since this is text input). Useful when narrowing the list and
        // picking a result for a palette action.
        KeyCode::Down => app.select_next(1),
        KeyCode::Up => app.select_prev(1),
        KeyCode::PageDown => app.select_next(app.tasks_per_page().max(1)),
        KeyCode::PageUp => app.select_prev(app.tasks_per_page().max(1)),
        KeyCode::Home => app.select_first(),
        KeyCode::End => app.select_last(),

        _ => {}
    }
    false
}
