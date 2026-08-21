//! Terminal setup and teardown: `run_tui` opens `/dev/tty`, enters the
//! alternate screen with mouse capture and the kitty keyboard protocol, builds
//! the `App`, runs the event loop, then restores the terminal on the way out.

use std::fs::OpenOptions;

use anyhow::Result;
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
use crate::state::Db;
use crate::tui::*;

use super::run::event_loop;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartupSyncPlan {
    launch_sync: bool,
    arm_refresh: bool,
    check_now: bool,
}

fn startup_sync_plan(sync_configured: bool, suppress_alert: bool) -> StartupSyncPlan {
    StartupSyncPlan {
        launch_sync: sync_configured,
        arm_refresh: sync_configured,
        // Always check now. Waiting for the startup sync meant the nudge could
        // take a slow pull's worth of seconds to appear — long enough to start
        // working and be interrupted by it. The armed refresh still reconciles:
        // if the sync shows triage was already done elsewhere, an open nudge is
        // withdrawn (see `App::reconcile_daily_triage_alert`).
        check_now: !suppress_alert,
    }
}

const fn periodic_pull_enabled(sync_configured: bool) -> bool {
    sync_configured
}

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

fn acquire_singleton_then_refresh(
    workspace: &crate::workspace::WorkspaceContext,
    refresh: impl FnOnce(&std::path::Path) -> Result<()>,
) -> Result<crate::tui::singleton::Guard> {
    let guard = crate::tui::singleton::Guard::acquire(workspace)?;
    refresh(workspace.root())?;
    Ok(guard)
}

fn load_startup_config(workspace: &crate::workspace::WorkspaceContext) -> Result<Config> {
    Config::try_load_for_startup(workspace)
}

fn register_server_lease(
    command_context: &crate::workspace::CommandContext,
) -> Result<crate::server::control::HeartbeatWorker> {
    let client = crate::server::control::ServerClient::default();
    let manifest = crate::workspace::WorkspaceManifest::load(
        command_context.workspace.root(),
        env!("CARGO_PKG_VERSION"),
    )?;
    let registration = crate::server::control::LeaseRegistration {
        generation: crate::server::lifecycle::ServerGeneration::new(),
        lease_id: crate::server::lifecycle::LeaseId::new(),
        workspace_id: command_context.workspace.id(),
        canonical_name: command_context.workspace.name().to_string(),
        ingress_id: manifest.receiver_ingress_id().into(),
        tui_pid: std::process::id(),
        resolved_root: command_context.workspace.root().to_path_buf(),
        job_socket: command_context.workspace.paths().job_socket(),
    };
    let mut registration = registration;
    client.connect_and_register(&mut registration)?;
    Ok(crate::server::control::HeartbeatWorker::start(
        client,
        registration,
    ))
}

pub(crate) fn run_tui(launch: TuiLaunch) -> Result<()> {
    let TuiLaunch {
        command_context,
        view,
        task_options,
        agent_kind,
        today,
        csv_path,
        all_tasks,
        all_habits,
        active_view,
        initial_search,
        skip_daily_triage_check,
    } = launch;
    let _singleton = acquire_singleton_then_refresh(
        &command_context.workspace,
        crate::command::server::refresh_agent_hooks,
    )?;
    crate::skills::migrate_global_skills_for_all_workspaces(Some(command_context.workspace.root()));
    // Reconcile embedded and user-authored project skills once, before any
    // app action can launch the brain panel's agent frontend.
    crate::skills::sync_for_startup(&command_context.workspace);
    let job_socket = crate::tui::singleton::JobSocket::bind(&command_context.workspace)?;
    let mut server_lease = register_server_lease(&command_context)?;
    let assignment = crate::tasks::task::assignment_context_for_workspace(
        &command_context.workspace,
        &command_context.actor,
    )?;
    let assignment_filter = crate::tasks::task::assignment_filter_for_startup(
        &assignment,
        task_options.assigned_to.as_deref(),
    )?;

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
    // a fresh instance id; the SessionStart integration reads BRAIN_* env vars
    // to attribute brain-panel sessions to this shell.
    let db = Db::open(&command_context.workspace)?;
    let config = load_startup_config(&command_context.workspace)?;
    // Best-effort maintenance before this shell touches anything: free
    // session locks held by tasks shells that have since died, so their
    // sessions become resumable. A failure here must never block startup.
    let _ = crate::agent::SessionStore::reap_dead_locks(&db);
    let instance = uuid::Uuid::new_v4().to_string();
    // Retain the root chosen once at workspace bootstrap; a later default
    // change cannot redirect this TUI.
    let brain_root = command_context.workspace.root().to_path_buf();

    let panel_side = db.get_panel_side();
    let search = build_search(&brain_root);
    let receiver_enabled = crate::command::server::receiver_enabled(&command_context)
        .unwrap_or_else(|error| {
            crate::logging::log(format!("receiver intent load failed: {error:#}"));
            false
        });
    let mut receiver = crate::tui::receiver::ReceiverRuntime::new(receiver_enabled);
    receiver.install_socket(job_socket);
    let mut app = App::new(AppInit {
        command_context: command_context.clone(),
        view,
        task_options,
        today,
        csv_path,
        all_tasks,
        all_habits,
        assignment,
        assignment_filter,
        active_view,
        initial_search,
        agenda_runner: Box::new(ZshFunctionRunner::new("agenda")),
        // The opener's stored command is unused — its `open(url)` default
        // shells `/usr/bin/open <url>` directly.
        open_runner: Box::new(ZshFunctionRunner::new("")),
        config,
        agent_kind,
        instance: instance.clone(),
        db,
        search,
        panel_side,
        skip_daily_triage_check,
        server_ingress: server_lease.ingress_id(),
        server_local_capability: server_lease.lease_id(),
        receiver,
    });
    crate::logging::log("workspace job socket and shared-server lease ready");
    // The brain panel opens at startup (resuming the latest session), but focus
    // stays on the tasks main view so `j`/`k` work immediately. `open_or_focus_
    // brain` focuses the panel, so flip focus back to the main view afterward.
    app.open_or_focus_brain(None);
    app.focus_tasks();

    // Auto-sync triggers (C4). All best-effort; none blocks the event loop.
    // Gated on `is_configured` so an unconfigured brain spawns neither a
    // startup child nor a watcher thread.
    let sync_cfg = crate::sync::config::SyncConfig::load(&command_context);
    let sync_configured = sync_cfg.is_configured();
    let startup_plan = startup_sync_plan(sync_configured, skip_daily_triage_check);

    // The daily-triage nudge must reflect *post-sync* state: another machine may
    // already have done or skipped today's triage, and that only reaches this
    // machine's `habits.csv` once the startup sync lands. So when a startup sync
    // is pending, DON'T show the modal now — the shell stays usable immediately.
    // Instead capture the journal baseline, kick the sync, and *arm the gate*;
    // `tick_triage_gate` runs the real check only once the sync completes. With
    // no startup sync, check right away.
    // The opt-out suppresses only the nudge. The gate still
    // refreshes config and task state after a successful startup pull.
    let seen = if startup_plan.arm_refresh {
        let seen =
            crate::sync::journal::Journal::open(&command_context.workspace.paths().sync_journal())
                .ok()
                .and_then(|j| j.latest_successful_downstream_id().ok())
                .flatten();
        Some(seen)
    } else {
        None
    };
    if startup_plan.launch_sync {
        let _ = crate::sync::trigger::spawn_detached_sync(
            &command_context.workspace,
            crate::sync::args::Direction::Pull,
        );
    }
    if let Some(seen) = seen {
        app.arm_triage_gate(seen, std::time::Instant::now());
    } else if startup_plan.check_now {
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
        crate::sync::watch::spawn_watcher(command_context.workspace.clone(), &sync_cfg).ok()
    } else {
        None
    };
    let periodic_puller = periodic_pull_enabled(sync_configured)
        .then(|| crate::sync::periodic::spawn_periodic_puller(command_context.workspace.clone()));
    let result = event_loop(&mut terminal, &mut app, &server_lease);

    if let Err(error) = server_lease.shutdown() {
        crate::logging::log(format!("shared-server lease unregister failed: {error:#}"));
    }
    app.shutdown_agent_controllers();

    drop(periodic_puller);
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

#[cfg(test)]
mod tests;
