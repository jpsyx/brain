//! Process-lifetime TUI execution, recurring work, and teardown ownership.

use anyhow::Result;

use crate::server::control::HeartbeatWorker;
use crate::sync::periodic::PeriodicPullHandle;
use crate::sync::watch::WatcherHandle;
use crate::tui::draw::draw;
use crate::tui::singleton::Guard;
use crate::tui::{App, TuiLaunch};

pub(super) mod builder;
mod shutdown;
pub(crate) mod terminal;
pub(super) mod tick;

use self::builder::RuntimeBuilder;
use self::shutdown::{RuntimeLifecycle, ShutdownStage};
use self::terminal::TerminalSession;

pub(crate) struct TuiRuntime {
    terminal: TerminalSession,
    app: App,
    server_lease: Option<HeartbeatWorker>,
    watcher: Option<WatcherHandle>,
    periodic_puller: Option<PeriodicPullHandle>,
    instance: String,
    lifecycle: RuntimeLifecycle,
    // Declared last so Rust drops the singleton after every other field.
    singleton: Guard,
}

impl TuiRuntime {
    pub(crate) fn start(launch: TuiLaunch) -> Result<Self> {
        RuntimeBuilder::new(launch).start()
    }

    pub(crate) fn run(&mut self) -> Result<()> {
        crate::tui::event_loop::event_loop(self)
    }

    pub(super) fn tick(&mut self) {
        if let Some(server_lease) = self.server_lease.as_ref() {
            tick::tick(&mut self.app, server_lease);
        }
    }

    pub(super) fn draw(&mut self) -> Result<()> {
        let terminal = &mut self.terminal;
        let app = &mut self.app;
        terminal.terminal_mut().draw(|frame| draw(frame, app))?;
        Ok(())
    }

    pub(super) const fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    pub(crate) fn shutdown(&mut self) -> Result<()> {
        let mut terminal_result = Ok(());
        for stage in self.lifecycle.begin_shutdown() {
            match stage {
                ShutdownStage::StopServerLease => {
                    if let Some(mut server_lease) = self.server_lease.take()
                        && let Err(error) = server_lease.shutdown()
                    {
                        crate::logging::log(format!(
                            "shared-server lease unregister failed: {error:#}"
                        ));
                    }
                }
                ShutdownStage::ShutdownAgentControllers => {
                    for error in self.app.shutdown_agent_controllers() {
                        crate::logging::log(format!("agent controller shutdown failed: {error}"));
                    }
                }
                ShutdownStage::StopPeriodicPuller => drop(self.periodic_puller.take()),
                ShutdownStage::StopWatcher => drop(self.watcher.take()),
                ShutdownStage::ReleaseSessionLock => {
                    if let Err(error) = self.app.services.release_session_lock(&self.instance) {
                        crate::logging::log(format!("session lock release failed: {error:#}"));
                    }
                }
                ShutdownStage::RestoreTerminal => terminal_result = self.terminal.restore(),
            }
        }
        terminal_result
    }
}

impl Drop for TuiRuntime {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            crate::logging::log(format!(
                "best-effort TUI runtime shutdown failed: {error:#}"
            ));
        }
        debug_assert!(self.lifecycle.singleton_held());
        self.lifecycle.begin_drop();
        let _ = &self.singleton;
    }
}
