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
        check_now: !sync_configured && !suppress_alert,
    }
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

#[allow(clippy::too_many_arguments)]
pub fn run_tui(
    command_context: &crate::workspace::CommandContext,
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
    skip_daily_triage_check: bool,
) -> Result<()> {
    let _singleton = acquire_singleton_then_refresh(
        &command_context.workspace,
        crate::command::server::refresh_agent_hooks,
    )?;
    let job_socket = crate::tui::singleton::JobSocket::bind(&command_context.workspace)?;
    let mut server_lease = register_server_lease(command_context)?;
    // First-run onboarding: seed personalization with a short skippable prompt
    // on the normal terminal, *before* we take over the screen. No-op when
    // already personalized or when there is no tty. Never blocks startup.
    crate::personalization::onboarding::maybe_run_first_time(&command_context.workspace);

    let assignment = crate::tasks::task::assignment_context_for_workspace(
        &command_context.workspace,
        &command_context.actor,
    )?;
    let assignment_filter = crate::tasks::task::assignment_filter_for_startup(
        &assignment,
        cli.filters.assigned_to.as_deref(),
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
    let mut app = App::new(
        command_context.clone(),
        view,
        cli,
        today,
        csv_path,
        all_tasks,
        all_habits,
        assignment,
        assignment_filter,
        active_view,
        initial_search,
        Box::new(ZshFunctionRunner::new("agenda")),
        // The opener's stored command is unused — its `open(url)` default
        // shells `/usr/bin/open <url>` directly.
        Box::new(ZshFunctionRunner::new("")),
        config,
        agent_kind,
        instance.clone(),
        db,
        search,
        panel_side,
        skip_daily_triage_check,
    );
    crate::logging::log("workspace job socket and shared-server lease ready");
    app.receiver_control = Some(job_socket);
    if with_receiver {
        app.start_receiver_server();
    }
    // The brain panel opens at startup (resuming the latest session), but focus
    // stays on the tasks main view so `j`/`k` work immediately. `open_or_focus_
    // brain` focuses the panel, so flip focus back to the main view afterward.
    app.open_or_focus_brain(None);
    app.focus_tasks();

    // Auto-sync triggers (C4). All best-effort; none blocks the event loop.
    // Gated on `is_configured` so an unconfigured brain spawns neither a
    // startup child nor a watcher thread.
    let sync_cfg = crate::sync::config::SyncConfig::load(command_context);
    let sync_configured = sync_cfg.is_configured();
    let startup_plan = startup_sync_plan(sync_configured, skip_daily_triage_check);

    // The daily-triage nudge must reflect *post-sync* state: another machine may
    // already have done or skipped today's triage, and that only reaches this
    // machine's `habits.csv` once the startup sync lands. So when a startup sync
    // is pending, DON'T show the modal now — the shell stays usable immediately.
    // Instead capture the journal baseline, kick the sync, and *arm the gate*;
    // `tick_triage_gate` runs the real check only once the sync completes. With
    // no startup sync, check right away.
    // `--no-daily-triage-check` suppresses only the nudge. The gate still
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
    let result = event_loop(&mut terminal, &mut app, &server_lease);

    if let Err(error) = server_lease.shutdown() {
        crate::logging::log(format!("shared-server lease unregister failed: {error:#}"));
    }
    app.shutdown_agent_controllers();

    // Local changes are already pushed by the watcher. Exit performs no pull
    // or timer-driven reconciliation; downstream sync happens only at startup
    // or at the receiver's two-hour freshness gate.
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
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::access::AccessMode;
    use crate::agent::{
        AgentFrontend, AgentSession, ClaudeFrontend, CodexFrontend, LaunchRequest, SessionPlan,
    };
    use crate::workspace::{WorkspaceContext, WorkspaceId, WorkspaceName};

    use super::{acquire_singleton_then_refresh, load_startup_config, startup_sync_plan};

    #[test]
    fn suppressed_startup_alert_still_waits_to_refresh_synced_state() {
        let plan = startup_sync_plan(true, true);

        assert!(plan.launch_sync);
        assert!(plan.arm_refresh);
        assert!(!plan.check_now);
    }

    #[test]
    fn held_workspace_singleton_prevents_hook_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("brain");
        std::fs::create_dir_all(&root).unwrap();
        let workspace = crate::workspace::WorkspaceContext::new(
            temp.path(),
            crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
            crate::workspace::WorkspaceName::parse("brain").unwrap(),
            &root,
            "pablo",
            temp.path(),
        )
        .unwrap();
        let _held = crate::tui::singleton::Guard::acquire(&workspace).unwrap();
        let marker = temp.path().join("refresh-ran");

        let result = acquire_singleton_then_refresh(&workspace, |_| {
            std::fs::write(&marker, b"ran")?;
            Ok(())
        });

        assert!(result.is_err());
        assert!(!marker.exists());
    }

    #[test]
    fn unrestricted_startup_ignores_malformed_unused_capability_fields_for_both_frontends() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("brain");
        std::fs::create_dir_all(root.join(".config")).unwrap();
        std::fs::write(
            root.join(".config/config.json"),
            r#"{
                "access_mode": "unrestricted",
                "allowed_mcps": "malformed",
                "allowed_skills": {"malformed": true},
                "enable_triage_habits": true
            }"#,
        )
        .unwrap();
        let workspace = Arc::new(
            WorkspaceContext::new(
                temp.path(),
                WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
                WorkspaceName::parse("brain").unwrap(),
                &root,
                "pablo",
                temp.path(),
            )
            .unwrap(),
        );

        let config = load_startup_config(&workspace).expect("unrestricted startup config");

        assert_eq!(config.access_mode, AccessMode::Unrestricted);
        for frontend in [
            Box::new(ClaudeFrontend::new(
                "claude",
                root,
                PathBuf::from("/unused/projects"),
            )) as Box<dyn AgentFrontend>,
            Box::new(CodexFrontend::new("codex")),
        ] {
            let request = LaunchRequest::from_trusted_context(
                Arc::clone(&workspace),
                crate::actor::test_actor("pablo"),
                SessionPlan::fresh(AgentSession::new("session-1").unwrap()),
                None,
                config.access_mode,
            );
            frontend
                .launch_spec(&request)
                .expect("unrestricted frontend startup spec");
        }
    }

    #[test]
    fn startup_remains_strict_for_access_mode_workspace_capabilities_and_live_fields() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("brain");
        std::fs::create_dir_all(root.join(".config")).unwrap();
        let workspace = WorkspaceContext::new(
            temp.path(),
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").unwrap(),
            WorkspaceName::parse("brain").unwrap(),
            &root,
            "pablo",
            temp.path(),
        )
        .unwrap();

        for body in [
            r#"{"access_mode":"invalid"}"#,
            r#"{"access_mode":"workspace_only","allowed_mcps":"malformed"}"#,
            r#"{"access_mode":"unrestricted","enable_triage_habits":"malformed"}"#,
        ] {
            std::fs::write(root.join(".config/config.json"), body).unwrap();
            assert!(load_startup_config(&workspace).is_err(), "accepted {body}");
        }
    }
}
