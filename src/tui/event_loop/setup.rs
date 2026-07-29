//! Terminal setup and teardown: `run_tui` opens `/dev/tty`, enters the
//! alternate screen with mouse capture and the kitty keyboard protocol, builds
//! the `App`, runs the event loop, then restores the terminal on the way out.

use std::{fs::OpenOptions, path::PathBuf};

use anyhow::Result;
use chrono::NaiveDate;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::config::Config;
use crate::session::AgentKind;
use crate::state::Db;
use crate::tasks::cli::Cli;
use crate::tasks::task::Task;
use crate::tasks::view::{View, ViewSpec};
use crate::tui::*;

use super::run::event_loop;

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
    agent_kind: AgentKind,
    today: NaiveDate,
    csv_path: PathBuf,
    all_tasks: Vec<Task>,
    all_habits: Vec<Task>,
    active_view: Option<View>,
    initial_search: Option<String>,
    with_receiver: bool,
) -> Result<()> {
    let _singleton = crate::tui::singleton::Guard::acquire()?;
    // First-run onboarding: seed personalization with a short skippable prompt
    // on the normal terminal, *before* we take over the screen. No-op when
    // already personalized or when there is no tty. Never blocks startup.
    crate::personalization::onboarding::maybe_run_first_time();

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
    // a fresh instance id; the Claude SessionStart hook reads BRAIN_* env vars
    // to attribute brain-panel Claude sessions to this shell.
    let db_path = Db::default_path();
    let db = Db::open(&db_path)?;
    let config = Config::load();
    // Best-effort maintenance before this shell touches anything: free
    // session locks held by tasks shells that have since died, so their
    // sessions become resumable. A failure here must never block startup.
    let _ = db.reap_dead_locks();
    let instance = uuid::Uuid::new_v4().to_string();
    // Honor the configured `root` (brain env), falling back to `$HOME/brain`
    // when it is unset or the resolved directory does not exist.
    let brain_root = crate::paths::brain_root().unwrap_or_else(|_| {
        std::env::var_os("HOME").map_or_else(
            || PathBuf::from("brain"),
            |h| PathBuf::from(h).join("brain"),
        )
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
        // The opener's stored command is unused — its `open(url)` default
        // shells `/usr/bin/open <url>` directly.
        Box::new(ZshFunctionRunner::new("")),
        config,
        agent_kind,
        instance.clone(),
        brain_root.clone(),
        db_path,
        db,
        search,
        panel_side,
    );
    app.receiver_control = crate::server::receiver::ControlSocket::bind().ok();
    if with_receiver {
        app.start_receiver_server();
    }
    // The brain panel opens at startup (resuming the latest session), but focus
    // stays on the tasks main view so `j`/`k` work immediately. `open_or_focus_
    // brain` focuses the panel, so flip focus back to the main view afterward.
    app.open_or_focus_brain(None);
    app.focus_tasks();

    // Auto-sync triggers (C4). All best-effort; none blocks the event loop.
    // Gated on `is_configured` so an unconfigured brain spawns no thread on start
    // and forks no detached child on exit (the triggers would no-op anyway).
    let sync_cfg = crate::sync::config::SyncConfig::load();
    let sync_configured = sync_cfg.is_configured();
    let startup_sync = sync_configured && sync_cfg.on_start;

    // The daily-triage nudge must reflect *post-sync* state: another machine may
    // already have done or skipped today's triage, and that only reaches this
    // machine's `habits.csv` once the startup sync lands. So when a startup sync
    // is pending, DON'T show the modal now — the shell stays usable immediately.
    // Instead capture the journal baseline, kick the sync, and *arm the gate*;
    // `tick_triage_gate` runs the real check once the sync completes (or a short
    // fail-open deadline passes). With no startup sync, check right away.
    if startup_sync {
        let seen =
            crate::sync::journal::Journal::open(&crate::sync::journal::Journal::default_path())
                .ok()
                .and_then(|j| j.latest_id().ok())
                .flatten();
        crate::sync::trigger::spawn_detached_sync(crate::sync::args::Direction::Both);
        app.arm_triage_gate(
            seen,
            std::time::Instant::now(),
            std::time::Duration::from_secs(10),
        );
    } else {
        // No sync coming — the local state is authoritative, so check now. The
        // confirm modal it may set renders on the very first frame.
        app.check_daily_triage();
    }
    // Anchor the triage re-check to the current logical day so a same-day
    // refresh (`r`) doesn't immediately re-fire the nudge; only crossing the
    // configured rollover hour into a new day does. Multi-day sessions get a
    // fresh check via `App::advance_triage_day` on each refresh.
    app.seed_triage_day(chrono::Local::now().naive_local());

    let watcher = if sync_cfg.watch_effective() {
        crate::sync::watch::spawn_watcher(&brain_root, &sync_cfg).ok()
    } else {
        None
    };
    let idle_puller = crate::sync::idle::spawn_idle_puller(&sync_cfg);

    let result = event_loop(&mut terminal, &mut app);

    // On exit, kick a detached final sync (it acquires the sync lock itself, and
    // coalesces if one is already running) and stop the watcher thread promptly.
    if sync_configured && sync_cfg.on_exit {
        crate::sync::trigger::spawn_detached_sync(crate::sync::args::Direction::Both);
    }
    drop(idle_puller);
    drop(watcher);

    // Release our session lock so the next open (this shell or another)
    // resumes the session this shell was driving.
    let _ = app.db.release(&instance);

    if enhanced_keyboard {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    result
}
