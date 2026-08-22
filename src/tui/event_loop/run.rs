//! The event loop: run one runtime tick, draw, poll for input, and dispatch
//! each keystroke through the fixed precedence: unconditional quit → modal
//! overlays → panel-close/new chords → focus/scroll chords → app-level view
//! switches → palette/brain/agenda accelerators → the focused panel/view.

use std::time::Duration as StdDuration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};

use crate::main_view::{self, MainView};
use crate::tui::*;

use super::modal_route::route_modal_key;

pub(in crate::tui) fn event_loop(runtime: &mut crate::tui::runtime::TuiRuntime) -> Result<()> {
    // Poll often enough that PTY output appears responsive without burning
    // CPU when idle. 50ms feels live to a typing user.
    let poll_interval = StdDuration::from_millis(50);
    loop {
        runtime.tick();
        runtime.draw()?;

        if !event::poll(poll_interval)? {
            continue;
        }
        let event = event::read()?;
        if update_application(runtime.app_mut(), &event) {
            return Ok(());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApplicationEvent {
    Ignore,
    Mouse(MouseEvent),
    Key(KeyEvent),
}

fn classify_application_event(event: &Event) -> ApplicationEvent {
    match event {
        Event::Mouse(mouse) => ApplicationEvent::Mouse(*mouse),
        Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
            ApplicationEvent::Key(*key)
        }
        Event::FocusGained
        | Event::FocusLost
        | Event::Key(_)
        | Event::Paste(_)
        | Event::Resize(_, _) => ApplicationEvent::Ignore,
    }
}

fn update_application(app: &mut App, event: &Event) -> bool {
    let k = match classify_application_event(event) {
        ApplicationEvent::Ignore => return false,
        ApplicationEvent::Mouse(mouse) => {
            handle_mouse(app, mouse);
            return false;
        }
        ApplicationEvent::Key(key) => key,
    };

    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let shift = k.modifiers.contains(KeyModifiers::SHIFT);

    // Ctrl+Q is the unconditional "quit the whole shell" accelerator,
    // resolved before modal routing and panel dispatch so nothing can
    // swallow it: it quits from either panel and even while a modal is
    // open. (Bare `q` / `Ctrl+C` stay contextual: they dismiss modals,
    // quit only from the tasks panel's normal mode, and are forwarded to
    // the agent in the brain panel.) 0x11, so no kitty-protocol dependency;
    // the caller releases the session lock and tears down the terminal on
    // this return.
    if ctrl_quits(k.code, ctrl) {
        return true;
    }

    // Any keystroke clears a transient flash from the previous action,
    // so the status line never lingers across user interactions.
    app.flash = None;

    // The vim-style count prefix only survives between consecutive
    // digit keystrokes and the `j`/`k`/↓/↑ motion that consumes them,
    // and only in the unmodal tasks panel. Any other action (a chord,
    // a modal key, a search keystroke, or a non-motion normal key) clears
    // it the moment it happens.
    let preserves_count = app.shell.focus() == Panel::Tasks
        && !app.tasks.is_searching()
        && app.overlay.is_none()
        && is_count_relevant_key(k.code, ctrl);
    if !preserves_count {
        app.tasks.clear_count();
    }

    // Modal overlays take all input, resolved before any panel / chord /
    // leader handling.
    if route_modal_key(app, &k, ctrl) {
        return false;
    }

    // Ctrl+X closes the brain panel (and ends its agent session) from
    // either panel. Intercepted before forwarding so it works even while
    // the brain panel is focused. No-op when no panel is open. 0x18, so
    // no kitty-protocol dependency. On a skill-session tab it closes only
    // that ephemeral session, leaving the main session untouched.
    if ctrl && matches!(k.code, KeyCode::Char('x' | 'X')) && app.any_brain_panel_visible() {
        if matches!(app.effective_brain_tab(), BrainTab::Session(_)) {
            app.close_active_skill_session();
        } else {
            app.close_brain();
        }
        return false;
    }

    // Ctrl+N starts a new agent session in the brain panel through the
    // selected adapter's semantic new-session sequence.
    // Intercepted before forwarding so it fires from either panel; only
    // while the panel is open (nothing to send to otherwise). 0x0E, so no
    // kitty-protocol dependency.
    if app.handle_new_session_shortcut(k.code, ctrl) {
        return false;
    }

    // Alt+H / Alt+L cycle panel focus. Alt+H always returns focus to the
    // tasks panel, the reliable way back from the brain panel, where
    // every other key (Space, arrows) is forwarded to the agent's input.
    // Alt+L focuses the brain panel when one is open (no-op otherwise).
    // We use Alt+letter rather than a Space leader or Alt+arrow because
    // both of those collide with editing inside the agent's prompt.
    // Alt+S opens the keyboard-shortcuts help modal. Bound to Alt+S (not a
    // bare key) so a literal `s` still types into the always-filtering
    // brain-search view; the Meta sequence is distinct on every terminal,
    // no kitty protocol needed.
    if main_view::alt_opens_help(k.code, alt) {
        open_overlay(&mut app.overlay, Overlay::Help(HelpState { scroll: 0 }));
        return false;
    }

    if alt {
        match k.code {
            KeyCode::Char('h' | 'H') => {
                app.focus_tasks();
                return false;
            }
            KeyCode::Char('l' | 'L') => {
                app.focus_brain();
                return false;
            }
            _ => {}
        }
    }
    // Alt+1 selects the main brain session and Alt+<n> the nth open skill
    // session, focusing the panel from either side. Handled before the
    // panel-key dispatch so they work while the brain panel is focused
    // (where a bare digit types into the agent). A digit with no tab behind
    // it is a no-op. Some macOS layouts surface the Option glyph instead of
    // an Alt-modified digit, which the classifier accepts.
    // A deliberate Alt chord is consumed either way (a tab request that
    // missed is still a tab request). A bare Option-produced glyph is also a
    // typeable character, so when it selects nothing it must fall through to
    // the panel rather than vanish.
    if let Some(slot) = alt_selects_brain_tab_slot(k.code, k.modifiers) {
        if app.select_brain_tab_slot(slot.index) || slot.from_chord {
            return false;
        }
    }
    // Alt+[ / Alt+] cycle the brain-panel tab (previous / next). The
    // reliable switch: terminal Alt+digit handling above is flaky, while
    // the bracket keys resolve either as Alt-modified brackets or the macOS
    // Option smart-quote glyphs. From either panel.
    if let Some(forward) = alt_cycles_brain_tab(k.code, k.modifiers) {
        app.cycle_brain_tab(forward);
        return false;
    }
    // Alt+U / Alt+D scroll the focused panel a half-page up / down.
    // Handled here (before the panel-key dispatch below forwards to the
    // child agent) so they work even while the brain panel is focused or
    // the search filter is active. Some terminals report macOS Option
    // glyphs instead of Alt-modified ASCII in richer keyboard modes.
    if let Some(up) = alt_scroll_direction(k.code, k.modifiers) {
        app.scroll_focused_half_page(up);
        return false;
    }

    // App-level main-view switching (main panel only, so the brain panel
    // keeps the agent's readline chords when it has focus): Ctrl+H / Ctrl+L
    // cycle left / right, Ctrl+T jumps to the tasks view, Ctrl+B jumps to
    // the brain-directory view. The brain panel stays open across a switch.
    if app.shell.focus() == Panel::Tasks {
        if let Some(dir) = main_view::ctrl_cycles_view(k.code, ctrl) {
            app.shell.cycle_main_view(dir);
            return false;
        }
        if let Some(mv) = main_view::ctrl_jumps_view(k.code, ctrl) {
            app.shell.show_main_view(mv);
            return false;
        }
    }

    // Ctrl+P opens the global command palette from the tasks panel only
    // (in the brain panel it's a readline binding for the child).
    if ctrl
        && ctrl_opens_palette(k.code)
        && app.shell.focus() == Panel::Tasks
        && matches!(app.shell.main_view(), MainView::Tasks | MainView::Logs)
    {
        let palette = if app.shell.main_view() == MainView::Logs {
            app.refresh_receiver_enabled();
            TaskPalette::new_logs_view(app.receiver.is_enabled())
        } else {
            app.refresh_receiver_enabled();
            let task_id = app.tasks.current_task_id();
            let is_habit = app.tasks.current_is_habit();
            let has_notes = app.tasks.current_has_notes();
            let notes_expanded = app.tasks.current_notes_expanded();
            let link_kind = app.tasks.selected_link_kind(&app.config.linear_base_url());
            TaskPalette::new(
                task_id,
                is_habit,
                has_notes,
                notes_expanded,
                link_kind,
                app.brain_panel_open(),
                app.log_path.is_some(),
            )
            .with_assignment_mode(app.tasks.assignment_snapshot().mode)
        };
        let receiver_enabled = app.receiver.is_enabled();
        let daily_triage_alert_disabled = app.skip_daily_triage_check;
        let (runnable_sessions, open_sessions) = app.skill_session_palette_rows();
        let palette = palette.with_runtime_context(
            receiver_enabled,
            daily_triage_alert_disabled,
            runnable_sessions,
            open_sessions,
        );
        open_overlay(&mut app.overlay, Overlay::TaskPalette(palette));
        return false;
    }

    // Ctrl+M (no Shift) opens (or focuses) the persistent brain panel,
    // resuming the shell's most-recently-active session. Note: many
    // terminals encode Ctrl+M identically to Enter (both → 0x0D), so this
    // only fires distinctly under the kitty keyboard protocol or
    // modifyOtherKeys; on default Terminal.app it collapses to
    // KeyCode::Enter and routes through Enter's handler instead. The
    // Shift-modified sibling Ctrl+Shift+M is the task-scoped message
    // (handled below).
    if ctrl_opens_brain(k.code, ctrl, shift) && app.shell.focus() == Panel::Tasks {
        app.open_or_focus_brain(None);
        return false;
    }

    // Ctrl+A: open today's agenda PDF via the user's `agenda` zsh
    // function (which generates the PDF on demand from
    // /tmp/<today>.md). When `agenda` reports "no markdown for
    // today", we fall back to a Yes/No modal offering to ask the
    // brain agent to generate it. Tasks-panel only; in the brain
    // panel Ctrl+A is the readline "beginning of line" binding and
    // we don't want to steal it from the child.
    if ctrl
        && matches!(k.code, KeyCode::Char('a' | 'A'))
        && app.shell.focus() == Panel::Tasks
        && app.shell.main_view() == MainView::Tasks
    {
        app.run_open_agenda();
        return false;
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
        && app.shell.focus() == Panel::Tasks
        && app.shell.main_view() == MainView::Tasks
    {
        let target = app.tasks.selected_identity();
        if let Some((id, label)) = target {
            open_overlay(
                &mut app.overlay,
                Overlay::BrainInput(BrainInputState::about(id, label)),
            );
        }
        return false;
    }

    match app.shell.focus() {
        // The brain panel routes to whichever tab is active: an ephemeral
        // skill session gets a plain forwarder; the main session keeps the
        // receiver/turn-aware handler.
        Panel::Brain => match app.effective_brain_tab() {
            BrainTab::Session(_) => handle_skill_session_key(app, &k, ctrl),
            BrainTab::Main => handle_brain_key(app, &k, ctrl),
        },
        // The main panel routes to whichever main view is showing. The
        // tasks view has its own normal/search modes; the brain-directory
        // view is an always-filtering picker.
        Panel::Tasks => match app.shell.main_view() {
            MainView::BrainSearch => {
                let effect = handle_search_view_key(&mut app.shell, &k, ctrl, alt);
                apply_search_view_effect(app, effect)
            }
            MainView::Logs => handle_logs_key(&mut app.shell, k.code, ctrl),
            MainView::Tasks if app.tasks.is_searching() => {
                match handle_search_key(&mut app.tasks, k.code, ctrl) {
                    TaskSearchEffect::None => false,
                    TaskSearchEffect::DelegateNormal => handle_normal_key(app, k.code, ctrl),
                }
            }
            MainView::Tasks => handle_normal_key(app, k.code, ctrl),
        },
    }
}

#[cfg(test)]
mod event_update_tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use super::{ApplicationEvent, classify_application_event};

    #[test]
    fn event_update_ignores_resize_and_key_release_but_accepts_key_press() {
        let released = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        let pressed = KeyEvent {
            kind: KeyEventKind::Press,
            ..released
        };

        assert_eq!(
            classify_application_event(&Event::Resize(80, 24)),
            ApplicationEvent::Ignore
        );
        assert_eq!(
            classify_application_event(&Event::Key(released)),
            ApplicationEvent::Ignore
        );
        assert_eq!(
            classify_application_event(&Event::Key(pressed)),
            ApplicationEvent::Key(pressed)
        );
    }
}
