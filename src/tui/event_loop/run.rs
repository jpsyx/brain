//! The event loop: run one runtime tick, draw, poll for input, and dispatch
//! each keystroke through the fixed precedence: unconditional quit → modal
//! overlays → panel-close/new chords → focus/scroll chords → app-level view
//! switches → palette/brain/agenda accelerators → the focused panel/view.
//!
//! Every accelerator here runs the same [`GlobalAction`] or
//! [`Command`](crate::tui::palette::Command) its palette row runs, so a key and
//! its row can never drift apart.

use std::time::Duration as StdDuration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};

use crate::main_view::{self, MainView};
use crate::tui::App;
use crate::tui::action::GlobalAction;
use crate::tui::handlers::{
    TaskSearchEffect, handle_brain_key, handle_logs_key, handle_mouse, handle_normal_key,
    handle_search_key, handle_session_tab_key,
};
use crate::tui::keymap::{
    alt_cycles_brain_tab, alt_scroll_direction, alt_selects_brain_tab_slot,
    ctrl_messages_brain_about_task, ctrl_opens_brain, ctrl_opens_palette, ctrl_quits,
    is_count_relevant_key,
};
use crate::tui::model::{BrainTab, Panel};
use crate::tui::overlay::{Overlay, open_overlay};
use crate::tui::palette::{Command, CommandPaletteState, TaskCommand};
use crate::tui::search_view::{apply_brain_dir_effect, handle_search_view_key};
use crate::tui::state::BrainDirView;
use crate::tui::tree_view::handle_tree_view_key;

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

pub(crate) fn update_application(app: &mut App, event: &Event) -> bool {
    let k = match classify_application_event(event) {
        ApplicationEvent::Ignore => return false,
        ApplicationEvent::Mouse(mouse) => {
            handle_mouse(app, mouse);
            return false;
        }
        ApplicationEvent::Key(key) => key,
    };

    // Ctrl+Q is the unconditional "quit the whole shell" accelerator,
    // resolved before modal routing and panel dispatch so nothing can
    // swallow it: it quits from either panel and even while a modal is
    // open. (Bare `q` / `Ctrl+C` stay contextual: they dismiss modals,
    // quit only from the tasks panel's normal mode, and are forwarded to
    // the agent in the brain panel.) 0x11, so no kitty-protocol dependency;
    // the caller releases the session lock and tears down the terminal on
    // this return.
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl_quits(k.code, ctrl) {
        return true;
    }

    // The palette's "Quit brain" row sets the same flag the chord returns, so
    // either route leaves through one door.
    dispatch_key(app, &k) || app.shell.take_quit_request()
}

fn dispatch_key(app: &mut App, k: &KeyEvent) -> bool {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let shift = k.modifiers.contains(KeyModifiers::SHIFT);

    // Any keystroke clears a transient flash from the previous action,
    // so the status line never lingers across user interactions.
    app.status.clear_flash();

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
    if route_modal_key(app, k, ctrl) {
        return false;
    }

    if main_view::esc_dismisses_error(k.code) && app.status.dismiss_error() {
        return false;
    }

    // Ctrl+X closes the active user-owned additional tab from either panel.
    // Main and lifecycle-owned receiver tabs ignore this chord.
    if ctrl && matches!(k.code, KeyCode::Char('x' | 'X')) && app.brain.any_panel_visible() {
        app.close_active_user_session();
        return false;
    }

    // Ctrl+N starts a new agent session in the brain panel through the
    // selected adapter's semantic new-session sequence.
    // Intercepted before forwarding so it fires from either panel; only
    // while the panel is open (nothing to send to otherwise). 0x0E, so no
    // kitty-protocol dependency.
    if ctrl && matches!(k.code, KeyCode::Char('n' | 'N')) && app.brain.any_panel_visible() {
        app.execute_global_action(GlobalAction::NewConversation);
        return false;
    }

    // Alt+S opens the keyboard-shortcuts help modal. Bound to Alt+S (not a
    // bare key) so a literal `s` still types into the always-filtering
    // brain-search view; the Meta sequence is distinct on every terminal,
    // no kitty protocol needed.
    if main_view::alt_opens_help(k.code, alt) {
        app.execute_global_action(GlobalAction::ShowShortcuts);
        return false;
    }

    // Alt+H / Alt+L cycle panel focus. Alt+H always returns focus to the
    // tasks panel, the reliable way back from the brain panel, where
    // every other key (Space, arrows) is forwarded to the agent's input.
    // Alt+L focuses the brain panel when one is open (no-op otherwise).
    // We use Alt+letter rather than a Space leader or Alt+arrow because
    // both of those collide with editing inside the agent's prompt.
    if alt {
        match k.code {
            KeyCode::Char('h' | 'H') => {
                app.execute_global_action(GlobalAction::FocusMainPanel);
                return false;
            }
            KeyCode::Char('l' | 'L') => {
                app.execute_global_action(GlobalAction::FocusBrainPanel);
                return false;
            }
            _ => {}
        }
    }
    // Alt+1 selects Main and Alt+<n> the corresponding additional tab,
    // focusing the panel from either side. Handled before the
    // panel-key dispatch so they work while the brain panel is focused
    // (where a bare digit types into the agent). A digit with no tab behind
    // it is a no-op. Some macOS layouts surface the Option glyph instead of
    // an Alt-modified digit, which the classifier accepts.
    // A deliberate Alt chord is consumed either way (a tab request that
    // missed is still a tab request). A bare Option-produced glyph is also a
    // typeable character, so when it selects nothing it must fall through to
    // the panel rather than vanish.
    if let Some(slot) = alt_selects_brain_tab_slot(k.code, k.modifiers)
        && (app.select_brain_tab_slot(slot.index) || slot.from_chord)
    {
        return false;
    }
    // Alt+[ / Alt+] cycle the brain-panel tab (previous / next). The
    // reliable switch: terminal Alt+digit handling above is flaky, while
    // the bracket keys resolve either as Alt-modified brackets or the macOS
    // Option smart-quote glyphs. From either panel.
    if let Some(forward) = alt_cycles_brain_tab(k.code, k.modifiers) {
        app.execute_global_action(GlobalAction::CycleBrainTab(forward));
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

    if app.shell.focus() == Panel::Tasks && main_panel_accelerator(app, k, ctrl, shift) {
        return false;
    }

    match app.shell.focus() {
        // The brain panel routes to whichever tab is active: an additional
        // session gets a plain forwarder; the main session keeps the
        // receiver/turn-aware handler.
        Panel::Brain => match app.effective_brain_tab() {
            BrainTab::Session(_) => handle_session_tab_key(app, k, ctrl),
            BrainTab::Main => handle_brain_key(app, k, ctrl),
        },
        // The main panel routes to whichever main view is showing. The
        // tasks view has its own normal/search modes; the brain-directory
        // view routes on to whichever sub-view is in front, the
        // always-filtering picker or the directory tree.
        Panel::Tasks => match app.shell.main_view() {
            MainView::BrainSearch => {
                let effect = match app.shell.brain_dir_view() {
                    BrainDirView::Search => handle_search_view_key(&mut app.shell, k, ctrl, alt),
                    BrainDirView::Tree => handle_tree_view_key(&mut app.shell, k, ctrl, alt),
                };
                apply_brain_dir_effect(app, effect)
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

/// The accelerators that only fire while the main panel has focus, so the
/// brain panel keeps the agent's own readline chords when it is focused.
/// Returns whether the key was consumed.
fn main_panel_accelerator(app: &mut App, k: &KeyEvent, ctrl: bool, shift: bool) -> bool {
    // Ctrl+H / Ctrl+L cycle the main view left / right; Ctrl+T, Ctrl+B and
    // Ctrl+E jump straight to one. The brain panel stays open across a switch.
    if let Some(dir) = main_view::ctrl_cycles_view(k.code, ctrl) {
        app.shell.cycle_main_view(dir);
        return true;
    }
    if let Some(view) = main_view::ctrl_jumps_view(k.code, ctrl) {
        match view {
            MainView::Tasks => app.execute_global_action(GlobalAction::ShowTasks),
            MainView::BrainSearch | MainView::Logs => {
                app.execute_global_action(GlobalAction::ShowBrainSearch);
            }
        }
        return true;
    }
    if main_view::ctrl_opens_explorer(k.code, ctrl) {
        app.execute_global_action(GlobalAction::OpenFileExplorer);
        return true;
    }

    // Ctrl+P opens the one global command palette, from whichever main view is
    // showing. In the brain panel it stays a readline binding for the child.
    if ctrl && ctrl_opens_palette(k.code) {
        let context = app.palette_context();
        open_overlay(
            &mut app.overlay,
            Overlay::CommandPalette(CommandPaletteState::new(&context)),
        );
        return true;
    }

    // Ctrl+M (no Shift) opens (or focuses) the persistent brain panel,
    // resuming the shell's most-recently-active session. Note: many terminals
    // encode Ctrl+M identically to Enter (both → 0x0D), so this only fires
    // distinctly under the kitty keyboard protocol; on default Terminal.app it
    // collapses to KeyCode::Enter and routes through Enter's handler instead.
    if ctrl_opens_brain(k.code, ctrl, shift) {
        app.execute_global_action(GlobalAction::MessageBrain);
        return true;
    }

    // Ctrl+A: open today's agenda, offering to generate it when missing. In
    // the brain panel Ctrl+A is the readline "beginning of line" binding and
    // we don't want to steal it from the child.
    if ctrl && matches!(k.code, KeyCode::Char('a' | 'A')) {
        app.execute_global_action(GlobalAction::OpenAgenda);
        return true;
    }

    // Ctrl+Shift+M: the task-scoped counterpart to Ctrl+M. Telling it apart
    // from the bare chord relies on the kitty protocol reporting Shift;
    // without it, Ctrl+Shift+M collapses to Enter and the palette is the
    // fallback. With nothing highlighted it asks which task, exactly as the
    // palette row does.
    if ctrl_messages_brain_about_task(k.code, ctrl, shift) {
        app.execute_command(Command::Task(TaskCommand::MessageBrainAbout));
        return true;
    }

    false
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
