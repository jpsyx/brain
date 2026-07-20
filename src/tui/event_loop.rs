//! Terminal setup (`run_tui`), the event loop, and modal key routing.

use super::*;

use std::{
    fs::OpenOptions,
    path::PathBuf,
    time::Duration as StdDuration,
};
use anyhow::Result;
use chrono::NaiveDate;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
};
use crate::tasks::cli::Cli;
use crate::config::Config;
use crate::main_view::{self, MainView};
use crate::state::Db;
use crate::tasks::task::Task;
use crate::tasks::view::{View, ViewSpec};

/// Turn off mouse *motion* reporting (DECSET 1002 button-drag + 1003 any-event)
/// that `EnableMouseCapture` also enables, keeping only button + wheel
/// reporting. With motion reporting on, iTerm2 won't let ⌘-hover / ⌘-click reach
/// its native link / Semantic-History handler; with only button + wheel, holding
/// ⌘ bypasses to native links while we still capture the scroll wheel.
/// Best-effort: a write failure just leaves the default capture in place.
fn disable_mouse_motion_reporting<W: std::io::Write>(w: &mut W) {
    let _ = w.write_all(b"\x1b[?1002l\x1b[?1003l");
    let _ = w.flush();
}

#[allow(clippy::too_many_arguments)]
pub fn run_tui(
    view: &ViewSpec,
    cli: &Cli,
    today: NaiveDate,
    csv_path: PathBuf,
    all_tasks: Vec<Task>,
    all_habits: Vec<Task>,
    active_view: Option<View>,
    initial_search: Option<String>,
) -> Result<()> {
    enable_raw_mode()?;
    // Render to /dev/tty, NOT stdout: the `brain` zsh wrapper captures this
    // binary's stdout (the shell-side plan), so writing the TUI to stdout
    // would send the alternate-screen escapes into that capture and hang the
    // wrapper on a blank line while the event loop runs forever. crossterm
    // reads key events from /dev/tty independently, so input still works.
    // (Matches the one-shot picker and the pre-merge brain shell.)
    let mut out = OpenOptions::new().write(true).open("/dev/tty")?;
    // EnableMouseCapture routes wheel events to us so we can scroll the
    // panels ourselves (the alternate screen has no native scrollback).
    // The cost: click-drag text selection now needs a modifier (Shift, or
    // Option in iTerm2 / Ghostty) to bypass mouse reporting.
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    disable_mouse_motion_reporting(&mut out);
    // Ask the terminal (best-effort) for the kitty keyboard protocol's
    // `DISAMBIGUATE_ESCAPE_CODES` extension. With it on, Ctrl+M is
    // reported distinctly from Enter, Ctrl+I from Tab, etc. — without
    // which they share encoding (both → 0x0D / 0x09) and shortcuts like
    // our Ctrl+M can't be distinguished from Enter. Terminals that don't
    // support the protocol ignore the sequence silently. iTerm2 3.5+,
    // Ghostty, WezTerm, Alacritty, Kitty, Foot all support it; the
    // legacy macOS Terminal.app does not.
    let enhanced_keyboard = execute!(
        out,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
    )
    .is_ok();
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    // Persistent state: open the session DB. Each tasks-shell invocation gets
    // a fresh instance id; the SessionStart hook reads `TASKS_INSTANCE_ID` to
    // attribute the brain panel's claude sessions to this shell.
    let db_path = Db::default_path();
    let db = Db::open(&db_path)?;
    let config = Config::load();
    // Best-effort maintenance before this shell touches anything: free
    // session locks held by tasks shells that have since died, so their
    // sessions become resumable. A failure here must never block startup.
    let _ = db.reap_dead_locks();
    let instance = uuid::Uuid::new_v4().to_string();
    // Honor the configured `root` (config.json), falling back to `$HOME/brain`
    // when it is unset or the resolved directory does not exist.
    let brain_root = crate::paths::brain_root().unwrap_or_else(|_| {
        std::env::var_os("HOME").map_or_else(|| PathBuf::from("brain"), |h| PathBuf::from(h).join("brain"))
    });

    let panel_side = db.get_panel_side();
    let search = build_search(&brain_root);
    let mut app = App::new(
        view,
        cli,
        today,
        csv_path,
        all_tasks,
        all_habits,
        active_view,
        initial_search,
        Box::new(ZshFunctionRunner::new("agenda")),
        Box::new(ZshFunctionRunner::new("habits")),
        // The opener's stored command is unused — its `open(url)` default
        // shells `/usr/bin/open <url>` directly.
        Box::new(ZshFunctionRunner::new("")),
        config,
        instance.clone(),
        brain_root,
        db_path,
        db,
        search,
        panel_side,
    );
    // The brain panel opens at startup (resuming the latest session), but focus
    // stays on the tasks main view so `j`/`k` work immediately. `open_or_focus_
    // brain` focuses the panel, so flip focus back to the main view afterward.
    app.open_or_focus_brain(None);
    app.focus_tasks();
    // Run the startup daily-triage check before entering the event
    // loop. The confirm modal it may set renders on the very first
    // frame so the user lands on the prompt rather than the tasks list.
    app.check_daily_triage();
    // Anchor the triage re-check to the current logical day so a same-day
    // refresh (`r`) doesn't immediately re-fire the nudge; only crossing the
    // configured rollover hour into a new day does. Multi-day sessions get a
    // fresh check via `App::advance_triage_day` on each refresh.
    app.seed_triage_day(chrono::Local::now().naive_local());
    let result = event_loop(&mut terminal, &mut app);

    // Release our session lock so the next open (this shell or another)
    // resumes the session this shell was driving.
    let _ = app.db.release(&instance);

    if enhanced_keyboard {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}
/// Which overlay should consume the next keystroke. Resolved before any
/// panel / chord / leader handling so an open modal is fully captive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalInput {
    Help,
    Palette,
    BrainInput,
    Confirm,
    LinkPicker,
    /// No modal is up — route to the panels (tasks / brain).
    Panels,
}

/// Which overlays are currently open. Mutually exclusive in practice.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ActiveModals {
    pub(crate) help: bool,
    pub(crate) palette: bool,
    pub(crate) brain_input: bool,
    pub(crate) confirm: bool,
    pub(crate) link_picker: bool,
}

/// Decide the keystroke target from the active overlays, in a fixed
/// precedence order.
pub(crate) const fn modal_input_target(m: ActiveModals) -> ModalInput {
    if m.help {
        ModalInput::Help
    } else if m.palette {
        ModalInput::Palette
    } else if m.brain_input {
        ModalInput::BrainInput
    } else if m.confirm {
        ModalInput::Confirm
    } else if m.link_picker {
        ModalInput::LinkPicker
    } else {
        ModalInput::Panels
    }
}

/// Route a keystroke to whichever modal overlay is active. Returns `true`
/// when a modal consumed the key (the caller should skip panel handling).
pub(crate) fn route_modal_key(app: &mut App<'_>, k: &crossterm::event::KeyEvent, ctrl: bool) -> bool {
    let target = modal_input_target(ActiveModals {
        help: app.help.is_some(),
        palette: app.palette.is_some(),
        brain_input: app.brain_input.is_some(),
        confirm: app.confirm.is_some(),
        link_picker: app.link_picker.is_some(),
    });
    match target {
        ModalInput::Help => handle_help_key(app, k, ctrl),
        ModalInput::Palette => handle_palette_key(app, k, ctrl),
        ModalInput::BrainInput => handle_brain_input_key(app, k, ctrl),
        ModalInput::Confirm => handle_confirm_key(app, k, ctrl),
        ModalInput::LinkPicker => handle_link_picker_key(app, k, ctrl),
        ModalInput::Panels => return false,
    }
    true
}

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

        // Ctrl+X closes the brain panel (and ends its claude session) from
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
                // Alt+U / Alt+D scroll the focused panel a half-page up / down.
                // Handled here (before the panel-key dispatch below forwards to
                // Claude) so they work even while the brain panel is focused or
                // the search filter is active — the brain panel scrolls its
                // scrollback, the tasks panel moves the selection.
                KeyCode::Char('u' | 'U') => {
                    app.scroll_focused_half_page(true);
                    continue;
                }
                KeyCode::Char('d' | 'D') => {
                    app.scroll_focused_half_page(false);
                    continue;
                }
                _ => {}
            }
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
