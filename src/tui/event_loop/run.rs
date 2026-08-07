//! The event loop: poll for input, fire deferred brain submits, and dispatch
//! each keystroke through the fixed precedence — unconditional quit → modal
//! overlays → panel-close/new chords → focus/scroll chords → app-level view
//! switches → palette/brain/agenda accelerators → the focused panel/view.

use std::time::Duration as StdDuration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::Backend};

use crate::main_view::{self, MainView};
use crate::tui::*;

use super::modal_route::route_modal_key;

pub(crate) fn event_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App<'_>,
    server_lease: &crate::server::control::HeartbeatWorker,
) -> Result<()> {
    // Poll often enough that PTY output appears responsive without burning
    // CPU when idle. 50ms feels live to a typing user.
    let poll_interval = StdDuration::from_millis(50);
    loop {
        // An agent exiting (e.g. the user's Ctrl-C, Ctrl-C inside it) closes the
        // brain panel — it does NOT quit the shell. Detected here so no extra
        // keystroke is needed and the closing Ctrl-C is never seen as a quit:
        // the two presses that quit the agent now auto-close the panel.
        app.close_exited_brain_panel();
        for event in server_lease.poll() {
            match event {
                crate::server::control::HeartbeatEvent::Recovered(generation) => {
                    crate::logging::log(format!(
                        "shared server recovered at generation {generation}"
                    ));
                }
                crate::server::control::HeartbeatEvent::RecoveryFailed(error) => {
                    crate::logging::log(format!("shared server recovery failed: {error}"));
                    app.flash = Some(FlashKind::Error(
                        "shared server unavailable; reconnecting".to_owned(),
                    ));
                }
            }
        }

        // Auto-close the ephemeral daily-triage tab when its session exits or
        // the `/triage` skill signals completion (matching one-time token).
        app.tick_triage_done();
        app.tick_receiver();
        app.tick_sync_status();

        // If the startup daily-triage nudge was deferred pending a background
        // sync, resolve it here once that sync lands:
        // reload the synced CSVs and show the modal only if triage is still due.
        app.tick_triage_gate();

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
        // the agent in the brain panel.) 0x11, so no kitty-protocol dependency;
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
            && app.assignee_filter.is_none()
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
        // no kitty-protocol dependency. On the daily-triage tab it closes only
        // that ephemeral session, leaving the main session untouched.
        if ctrl && matches!(k.code, KeyCode::Char('x' | 'X')) && app.any_brain_panel_visible() {
            if app.effective_brain_tab() == BrainTab::Triage {
                app.close_triage_tab();
            } else {
                app.close_brain();
            }
            continue;
        }

        // Ctrl+N starts a new agent session in the brain panel through the
        // selected adapter's semantic new-session sequence.
        // Intercepted before forwarding so it fires from either panel; only
        // while the panel is open (nothing to send to otherwise). 0x0E, so no
        // kitty-protocol dependency.
        if app.handle_new_session_shortcut(k.code, ctrl) {
            continue;
        }

        // Alt+H / Alt+L cycle panel focus. Alt+H always returns focus to the
        // tasks panel — the reliable way back from the brain panel, where
        // every other key (Space, arrows) is forwarded to the agent's input.
        // Alt+L focuses the brain panel when one is open (no-op otherwise).
        // We use Alt+letter rather than a Space leader or Alt+arrow because
        // both of those collide with editing inside the agent's prompt.
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
        // Alt+1 / Alt+2 select the brain-panel tab (main session / ephemeral
        // daily-triage session) and focus the panel, from either side. Handled
        // before the panel-key dispatch so they work while the brain panel is
        // focused (where a bare digit types into the agent). Alt+2 is a no-op
        // when no triage tab is open. Some macOS layouts surface the Option
        // glyph instead of an Alt-modified digit, which the classifier accepts.
        if let Some(tab) = alt_selects_brain_tab(k.code, k.modifiers) {
            app.select_brain_tab(tab);
            continue;
        }
        // Alt+[ / Alt+] cycle the brain-panel tab (previous / next). The
        // reliable switch — terminal Alt+digit handling above is flaky, while
        // the bracket keys resolve either as Alt-modified brackets or the macOS
        // Option smart-quote glyphs. From either panel.
        if let Some(forward) = alt_cycles_brain_tab(k.code, k.modifiers) {
            app.cycle_brain_tab(forward);
            continue;
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
        // keeps the agent's readline chords when it has focus): Ctrl+H / Ctrl+L
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
            && matches!(app.main_view, MainView::Tasks | MainView::Logs)
        {
            app.palette = if app.main_view == MainView::Logs {
                app.refresh_receiver_enabled();
                Some(PaletteState::new_logs_view(app.receiver_enabled))
            } else {
                app.refresh_receiver_enabled();
                let task_id = app.current_task_id();
                let is_habit = app.current_is_habit();
                let has_notes = app.current_has_notes();
                let notes_expanded = app.current_notes_expanded();
                let link_kind = app.current_link_kind();
                Some(
                    PaletteState::new(
                        task_id,
                        is_habit,
                        has_notes,
                        notes_expanded,
                        link_kind,
                        app.brain_panel_open(),
                        app.log_path.is_some(),
                    )
                    .with_assignment_mode(app.assignment.mode()),
                )
            };
            let receiver_enabled = app.receiver_enabled;
            let daily_triage_alert_disabled = app.skip_daily_triage_check;
            let triage_open = app.triage_brain.is_some();
            if let Some(palette) = app.palette.as_mut() {
                palette.receiver_enabled = receiver_enabled;
                palette.daily_triage_alert_disabled = daily_triage_alert_disabled;
                palette.triage_open = triage_open;
            }
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
            // The brain panel routes to whichever tab is active: the ephemeral
            // daily-triage session gets a plain forwarder; the main session
            // keeps the receiver/turn-aware handler.
            Panel::Brain => match app.effective_brain_tab() {
                BrainTab::Triage => handle_triage_key(app, &k, ctrl),
                BrainTab::Main => handle_brain_key(app, &k, ctrl),
            },
            // The main panel routes to whichever main view is showing. The
            // tasks view has its own normal/search modes; the brain-directory
            // view is an always-filtering picker.
            Panel::Tasks => match app.main_view {
                MainView::BrainSearch => handle_search_view_key(app, &k, ctrl, alt),
                MainView::Logs => handle_logs_key(app, k.code, ctrl),
                MainView::Tasks if app.in_search => handle_search_key(app, k.code, ctrl),
                MainView::Tasks => handle_normal_key(app, k.code, ctrl),
            },
        };
        if quit {
            return Ok(());
        }
    }
}
