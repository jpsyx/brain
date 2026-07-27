//! The event loop: poll for input, fire deferred brain submits, and dispatch
//! each keystroke through the fixed precedence — unconditional quit → modal
//! overlays → panel-close/new chords → focus/scroll chords → app-level view
//! switches → palette/brain/agenda accelerators → the focused panel/view.

use std::time::Duration as StdDuration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{backend::Backend, Terminal};

use crate::main_view::{self, MainView};
use crate::tui::*;

use super::modal_route::route_modal_key;

pub(crate) fn event_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App<'_>) -> Result<()> {
    // Poll often enough that PTY output appears responsive without burning
    // CPU when idle. 50ms feels live to a typing user.
    let poll_interval = StdDuration::from_millis(50);
    loop {
        // claude exiting (e.g. the user's Ctrl-C, Ctrl-C inside it) closes the
        // brain panel — it does NOT quit the shell. Detected here so no extra
        // keystroke is needed and the closing Ctrl-C is never seen as a quit:
        // the two presses that quit claude now auto-close the panel.
        if app.brain.as_ref().is_some_and(|p| !p.is_alive()) {
            app.close_brain();
        }

        // Fire any deferred submitting Return for a freshly-seeded prompt. Runs
        // every iteration (including idle 50ms polls) so the Enter lands a
        // couple of ticks after the text, letting claude submit it.
        app.tick_brain_submit();

        terminal.draw(|f| draw(f, app))?;

        if !event::poll(poll_interval)? {
            continue;
        }
        let ev = event::read()?;

        // Resize: ratatui's draw() handles its own layout; the brain PTY
        // resize is done inside draw() once we know the right panel's Rect.
        if matches!(ev, Event::Resize(_, _)) {
            continue;
        }

        // Mouse wheel: scroll whichever panel the cursor is over. Handled
        // here (before key routing) and never forwarded to the brain PTY.
        if let Event::Mouse(me) = ev {
            handle_mouse(app, me);
            continue;
        }

        let Event::Key(k) = ev else {
            continue;
        };
        if k.kind != KeyEventKind::Press && k.kind != KeyEventKind::Repeat {
            continue;
        }

        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);

        // Ctrl+Q is the unconditional "quit the whole shell" accelerator,
        // resolved before modal routing and panel dispatch so nothing can
        // swallow it: it quits from either panel and even while a modal is
        // open. (Bare `q` / `Ctrl+C` stay contextual — they dismiss modals,
        // quit only from the tasks panel's normal mode, and are forwarded to
        // claude in the brain panel.) 0x11, so no kitty-protocol dependency;
        // the caller releases the session lock and tears down the terminal on
        // this return.
        if ctrl_quits(k.code, ctrl) {
            return Ok(());
        }

        // Any keystroke clears a transient flash from the previous action,
        // so the status line never lingers across user interactions.
        app.flash = None;

        // The vim-style count prefix only survives between consecutive
        // digit keystrokes and the `j`/`k`/↓/↑ motion that consumes them,
        // and only in the unmodal tasks panel. Any other action — a chord,
        // a modal key, a search keystroke, a non-motion normal key —
        // clears it the moment it happens.
        let preserves_count = app.focus == Panel::Tasks
            && !app.in_search
            && app.palette.is_none()
            && app.brain_input.is_none()
            && app.confirm.is_none()
            && app.link_picker.is_none()
            && app.help.is_none()
            && is_count_relevant_key(k.code, ctrl);
        if !preserves_count {
            app.pending_count = None;
        }

        // Modal overlays take all input, resolved before any panel / chord /
        // leader handling.
        if route_modal_key(app, &k, ctrl) {
            continue;
        }

        // Ctrl+X closes the brain panel (and ends its agent session) from
        // either panel. Intercepted before forwarding so it works even while
        // the brain panel is focused. No-op when no panel is open. 0x18, so
        // no kitty-protocol dependency.
        if ctrl && matches!(k.code, KeyCode::Char('x' | 'X')) && app.brain_panel_open() {
            app.close_brain();
            continue;
        }

        // Ctrl+N starts a new Claude session in the brain panel: type `/new`
        // into the running conversation and submit it (via the same deferred-
        // Return path as a seeded prompt, so claude doesn't paste the `\r`).
        // Intercepted before forwarding so it fires from either panel; only
        // while the panel is open (nothing to send to otherwise). 0x0E, so no
        // kitty-protocol dependency.
        if ctrl && matches!(k.code, KeyCode::Char('n' | 'N')) && app.brain_panel_open() {
            app.open_or_focus_brain(Some("/new"));
            continue;
        }

        // Alt+H / Alt+L cycle panel focus. Alt+H always returns focus to the
        // tasks panel — the reliable way back from the brain panel, where
        // every other key (Space, arrows) is forwarded to Claude's input.
        // Alt+L focuses the brain panel when one is open (no-op otherwise).
        // We use Alt+letter rather than a Space leader or Alt+arrow because
        // both of those collide with editing inside Claude's prompt.
        // Alt+S opens the keyboard-shortcuts help modal. Bound to Alt+S (not a
        // bare key) so a literal `s` still types into the always-filtering
        // brain-search view; the Meta sequence is distinct on every terminal,
        // no kitty protocol needed.
        if main_view::alt_opens_help(k.code, alt) {
            app.help = Some(HelpState { scroll: 0 });
            continue;
        }

        if alt {
            match k.code {
                KeyCode::Char('h' | 'H') => {
                    app.focus_tasks();
                    continue;
                }
                KeyCode::Char('l' | 'L') => {
                    app.focus_brain();
                    continue;
                }
                _ => {}
            }
        }
        // Alt+U / Alt+D scroll the focused panel a half-page up / down.
        // Handled here (before the panel-key dispatch below forwards to the
        // child agent) so they work even while the brain panel is focused or
        // the search filter is active. Some terminals report macOS Option
        // glyphs instead of Alt-modified ASCII in richer keyboard modes.
        if let Some(up) = alt_scroll_direction(k.code, k.modifiers) {
            app.scroll_focused_half_page(up);
            continue;
        }

        // App-level main-view switching (main panel only, so the brain panel
        // keeps Claude's readline chords when it has focus): Ctrl+H / Ctrl+L
        // cycle left / right, Ctrl+T jumps to the tasks view, Ctrl+B jumps to
        // the brain-directory view. The brain panel stays open across a switch.
        if app.focus == Panel::Tasks {
            if let Some(dir) = main_view::ctrl_cycles_view(k.code, ctrl) {
                app.main_view = app.main_view.step(dir);
                continue;
            }
            if let Some(mv) = main_view::ctrl_jumps_view(k.code, ctrl) {
                app.main_view = mv;
                continue;
            }
        }

        // Ctrl+P opens the global command palette from the tasks panel only
        // (in the brain panel it's a readline binding for the child).
        if ctrl
            && ctrl_opens_palette(k.code)
            && app.focus == Panel::Tasks
            && app.main_view == MainView::Tasks
        {
            let task_id = app.current_task_id();
            let is_habit = app.current_is_habit();
            let has_notes = app.current_has_notes();
            let notes_expanded = app.current_notes_expanded();
            let link_kind = app.current_link_kind();
            app.palette = Some(PaletteState::new(
                task_id,
                is_habit,
                has_notes,
                notes_expanded,
                link_kind,
                app.brain_panel_open(),
                app.log_path.is_some(),
            ));
            continue;
        }

        // Ctrl+M (no Shift) opens (or focuses) the persistent brain panel,
        // resuming the shell's most-recently-active session. Note: many
        // terminals encode Ctrl+M identically to Enter (both → 0x0D), so this
        // only fires distinctly under the kitty keyboard protocol or
        // modifyOtherKeys; on default Terminal.app it collapses to
        // KeyCode::Enter and routes through Enter's handler instead. The
        // Shift-modified sibling Ctrl+Shift+M is the task-scoped message
        // (handled below).
        if ctrl_opens_brain(k.code, ctrl, shift) && app.focus == Panel::Tasks {
            app.open_or_focus_brain(None);
            continue;
        }

        // Ctrl+A: open today's agenda PDF via the user's `agenda` zsh
        // function (which generates the PDF on demand from
        // /tmp/<today>.md). When `agenda` reports "no markdown for
        // today", we fall back to a Yes/No modal offering to ask the
        // brain agent to generate it. Tasks-panel only — in the brain
        // panel Ctrl+A is the readline "beginning of line" binding and
        // we don't want to steal it from the child.
        if ctrl
            && matches!(k.code, KeyCode::Char('a' | 'A'))
            && app.focus == Panel::Tasks
            && app.main_view == MainView::Tasks
        {
            app.run_open_agenda();
            continue;
        }

        // (Ctrl+H is now the "cycle main view left" accelerator, handled
        // above. Opening the habits page in the browser moved to the command
        // palette's "Open habits page" row.)

        // Ctrl+Shift+M: task-scoped counterpart to Ctrl+M. Opens the input
        // modal preloaded with the highlighted task as context so the brain
        // agent knows which task the message is about. No-op when nothing is
        // selected. Distinguishing it from the bare Ctrl+M panel toggle relies
        // on the kitty protocol reporting the Shift modifier; without it,
        // Ctrl+Shift+M collapses to Enter and the palette is the fallback.
        if ctrl_messages_brain_about_task(k.code, ctrl, shift)
            && app.focus == Panel::Tasks
            && app.main_view == MainView::Tasks
        {
            let target = app
                .selected_task
                .and_then(|i| app.visible_tasks.get(i))
                .map(|t| (t.id.clone(), t.name.clone()));
            if let Some((id, label)) = target {
                app.brain_input = Some(BrainInputState::about(id, label));
            }
            continue;
        }

        let quit = match app.focus {
            Panel::Brain => handle_brain_key(app, &k, ctrl),
            // The main panel routes to whichever main view is showing. The
            // tasks view has its own normal/search modes; the brain-directory
            // view is an always-filtering picker.
            Panel::Tasks => match app.main_view {
                MainView::BrainSearch => handle_search_view_key(app, &k, ctrl, alt),
                MainView::Tasks if app.in_search => handle_search_key(app, k.code, ctrl),
                MainView::Tasks => handle_normal_key(app, k.code, ctrl),
            },
        };
        if quit {
            return Ok(());
        }
    }
}
