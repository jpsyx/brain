//! Key handlers for the tasks main view: `handle_normal_key` (scroll /
//! navigation / view shortcuts / task actions) and `handle_search_key` (the
//! `/` fuzzy-filter input mode, which delegates ctrl chords back to normal).

use crate::tui::*;

use crossterm::event::KeyCode;

use crate::tasks::view::View;

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
                app.clear_active_filters();
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
                )
                .with_assignment_controls(app.assignment.mode().show_reassign_control));
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
