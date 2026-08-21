//! Process-lifetime TUI startup, recurring work, and resource ownership.

use anyhow::{Context, Result};

use crate::config::Config;
use crate::server::control::HeartbeatWorker;
use crate::state::Db;
use crate::sync::periodic::PeriodicPullHandle;
use crate::sync::watch::WatcherHandle;
use crate::tui::singleton::{Guard, JobSocket};
use crate::tui::{App, AppInit, TuiLaunch, ZshFunctionRunner, build_search, draw};

mod shutdown;
pub(crate) mod terminal;
pub(super) mod tick;

use self::shutdown::{AcquisitionStage, RuntimeLifecycle, ShutdownStage, StartupResources};
use self::terminal::TerminalSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StartupSyncPlan {
    pub(super) launch_sync: bool,
    pub(super) arm_refresh: bool,
    pub(super) check_now: bool,
}

pub(super) const fn startup_sync_plan(
    sync_configured: bool,
    suppress_alert: bool,
) -> StartupSyncPlan {
    StartupSyncPlan {
        launch_sync: sync_configured,
        arm_refresh: sync_configured,
        check_now: !suppress_alert,
    }
}

pub(super) const fn periodic_pull_enabled(sync_configured: bool) -> bool {
    sync_configured
}

pub(super) fn acquire_singleton_then_refresh(
    workspace: &crate::workspace::WorkspaceContext,
    refresh: impl FnOnce(&std::path::Path) -> Result<()>,
) -> Result<Guard> {
    let guard = Guard::acquire(workspace)?;
    refresh(workspace.root())?;
    Ok(guard)
}

pub(super) fn load_startup_config(
    workspace: &crate::workspace::WorkspaceContext,
) -> Result<Config> {
    Config::try_load_for_startup(workspace)
}

fn register_server_lease(
    command_context: &crate::workspace::CommandContext,
) -> Result<HeartbeatWorker> {
    let client = crate::server::control::ServerClient::default();
    let manifest = crate::workspace::WorkspaceManifest::load(
        command_context.workspace.root(),
        env!("CARGO_PKG_VERSION"),
    )?;
    let mut registration = crate::server::control::LeaseRegistration {
        generation: crate::server::lifecycle::ServerGeneration::new(),
        lease_id: crate::server::lifecycle::LeaseId::new(),
        workspace_id: command_context.workspace.id(),
        canonical_name: command_context.workspace.name().to_string(),
        ingress_id: manifest.receiver_ingress_id().into(),
        tui_pid: std::process::id(),
        resolved_root: command_context.workspace.root().to_path_buf(),
        job_socket: command_context.workspace.paths().job_socket(),
    };
    client.connect_and_register(&mut registration)?;
    Ok(HeartbeatWorker::start(client, registration))
}

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
                    if let Err(error) = self.app.db.release(&self.instance) {
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

struct RuntimeBuilder {
    launch: Option<TuiLaunch>,
    lifecycle: RuntimeLifecycle,
}

struct PreparedRuntime {
    terminal: TerminalSession,
    app: App,
    watcher: Option<WatcherHandle>,
    periodic_puller: Option<PeriodicPullHandle>,
    instance: String,
}

impl RuntimeBuilder {
    const fn new(launch: TuiLaunch) -> Self {
        Self {
            launch: Some(launch),
            lifecycle: RuntimeLifecycle::new(),
        }
    }

    fn start(mut self) -> Result<TuiRuntime> {
        let singleton = self.acquire_workspace_boundary()?;
        let job_socket = self.bind_receiver_endpoint()?;
        let server_lease = self.start_server_lease()?;
        let startup_resources = StartupResources::new(server_lease, job_socket);
        let prepared =
            startup_resources.prepare(|server_lease| self.prepare_runtime(server_lease))?;
        let lifecycle = self.lifecycle;
        Ok(prepared.finish(|prepared, server_lease, job_socket| {
            let PreparedRuntime {
                terminal,
                mut app,
                watcher,
                periodic_puller,
                instance,
            } = prepared;
            app.receiver.install_socket(job_socket);
            TuiRuntime {
                terminal,
                app,
                server_lease: Some(server_lease),
                watcher,
                periodic_puller,
                instance,
                lifecycle,
                singleton,
            }
        }))
    }

    fn prepare_runtime(&mut self, server_lease: &HeartbeatWorker) -> Result<PreparedRuntime> {
        let assignment = self.prepare_assignment()?;
        let terminal = self.acquire_terminal()?;
        let (mut app, instance) = self.build_application(server_lease, assignment)?;
        Self::launch_initial_agent_panel(&mut app);
        let (watcher, periodic_puller) = self.start_sync_services(&mut app)?;
        anyhow::ensure!(
            self.lifecycle.is_running(),
            "TUI runtime startup is incomplete"
        );
        Ok(PreparedRuntime {
            terminal,
            app,
            watcher,
            periodic_puller,
            instance,
        })
    }

    fn launch(&self) -> Result<&TuiLaunch> {
        self.launch
            .as_ref()
            .context("TUI launch request was already consumed")
    }

    fn acquire_workspace_boundary(&mut self) -> Result<Guard> {
        let workspace = &self.launch()?.command_context.workspace;
        let singleton =
            acquire_singleton_then_refresh(workspace, crate::command::server::refresh_agent_hooks)?;
        crate::skills::migrate_global_skills_for_all_workspaces(Some(workspace.root()));
        crate::skills::sync_for_startup(workspace);
        self.lifecycle
            .record_acquired(AcquisitionStage::WorkspaceSingleton)?;
        Ok(singleton)
    }

    fn bind_receiver_endpoint(&mut self) -> Result<JobSocket> {
        let socket = JobSocket::bind(&self.launch()?.command_context.workspace)?;
        self.lifecycle
            .record_acquired(AcquisitionStage::ReceiverEndpoint)?;
        Ok(socket)
    }

    fn start_server_lease(&mut self) -> Result<HeartbeatWorker> {
        let lease = register_server_lease(&self.launch()?.command_context)?;
        self.lifecycle
            .record_acquired(AcquisitionStage::ServerLease)?;
        Ok(lease)
    }

    fn prepare_assignment(
        &self,
    ) -> Result<(
        crate::tasks::task::AssignmentContext,
        Option<crate::users::UserId>,
    )> {
        let launch = self.launch()?;
        let assignment = crate::tasks::task::assignment_context_for_workspace(
            &launch.command_context.workspace,
            &launch.command_context.actor,
        )?;
        let filter = crate::tasks::task::assignment_filter_for_startup(
            &assignment,
            launch.task_options.assigned_to.as_deref(),
        )?;
        Ok((assignment, filter))
    }

    fn acquire_terminal(&mut self) -> Result<TerminalSession> {
        let terminal = TerminalSession::acquire()?;
        self.lifecycle.record_acquired(AcquisitionStage::Terminal)?;
        Ok(terminal)
    }

    fn build_application(
        &mut self,
        server_lease: &HeartbeatWorker,
        assignment: (
            crate::tasks::task::AssignmentContext,
            Option<crate::users::UserId>,
        ),
    ) -> Result<(App, String)> {
        let launch = self
            .launch
            .take()
            .context("TUI launch request was already consumed")?;
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
        let db = Db::open(&command_context.workspace)?;
        let config = load_startup_config(&command_context.workspace)?;
        let _ = crate::agent::SessionStore::reap_dead_locks(&db);
        let instance = uuid::Uuid::new_v4().to_string();
        let brain_root = command_context.workspace.root().to_path_buf();
        let panel_side = db.get_panel_side();
        let search = build_search(&brain_root);
        let receiver_enabled = crate::command::server::receiver_enabled(&command_context)
            .unwrap_or_else(|error| {
                crate::logging::log(format!("receiver intent load failed: {error:#}"));
                false
            });
        let receiver = crate::tui::receiver::ReceiverRuntime::new(receiver_enabled);
        let app = App::new(AppInit {
            command_context,
            view,
            task_options,
            today,
            csv_path,
            all_tasks,
            all_habits,
            assignment: assignment.0,
            assignment_filter: assignment.1,
            active_view,
            initial_search,
            agenda_runner: Box::new(ZshFunctionRunner::new("agenda")),
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
        self.lifecycle
            .record_acquired(AcquisitionStage::Application)?;
        Ok((app, instance))
    }

    fn launch_initial_agent_panel(app: &mut App) {
        crate::logging::log("workspace job socket and shared-server lease ready");
        app.open_or_focus_brain(None);
        app.focus_tasks();
    }

    fn start_sync_services(
        &mut self,
        app: &mut App,
    ) -> Result<(Option<WatcherHandle>, Option<PeriodicPullHandle>)> {
        let workspace = app.command_context.workspace.clone();
        let sync_config = crate::sync::config::SyncConfig::load(&app.command_context);
        let sync_configured = sync_config.is_configured();
        let plan = startup_sync_plan(sync_configured, app.skip_daily_triage_check);
        let seen = plan.arm_refresh.then(|| {
            crate::sync::journal::Journal::open(&workspace.paths().sync_journal())
                .ok()
                .and_then(|journal| journal.latest_successful_downstream_id().ok())
                .flatten()
        });
        if plan.launch_sync {
            let _ = crate::sync::trigger::spawn_detached_sync(
                &workspace,
                crate::sync::args::Direction::Pull,
            );
        }
        if let Some(seen) = seen {
            app.arm_triage_gate(seen, std::time::Instant::now());
        } else if plan.check_now {
            app.check_daily_triage();
        }
        app.seed_triage_day(chrono::Local::now().naive_local());

        let watcher = if sync_config.watch_effective() {
            crate::sync::watch::spawn_watcher(workspace.clone(), &sync_config).ok()
        } else {
            None
        };
        let periodic_puller = periodic_pull_enabled(sync_configured)
            .then(|| crate::sync::periodic::spawn_periodic_puller(workspace));
        self.lifecycle
            .record_acquired(AcquisitionStage::BackgroundServices)?;
        Ok((watcher, periodic_puller))
    }
}
