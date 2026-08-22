//! `App::new`: assemble the initial shell state from the owned launch request,
//! loaded task/habit lists, injected runners, and the session DB, then build
//! the first body.

use std::path::PathBuf;

use chrono::NaiveDate;

use crate::config::Config;
use crate::session::AgentKind;
use crate::state::Db;
use crate::tasks::task::Task;
use crate::tasks::view::View;
use crate::tui::app_sync::{ReceiverSyncRuntime, SystemReceiverSyncRuntime};
use crate::tui::shell::ShellRunner;
use crate::tui::state::{
    AppContext, AppContextInit, AppServices, AppServicesInit, BrainPanelState, BrainPanelStateInit,
    ShellState, StatusState, StatusStateInit, TasksState, TasksStateInit,
};
use crate::tui::status_warning::receiver_phone_warning;
use crate::tui::{App, PanelSide};

pub(crate) struct AppInit {
    pub(crate) command_context: crate::workspace::CommandContext,
    pub(crate) view: crate::tasks::view::ViewSpec,
    pub(crate) task_options: crate::tasks::view::TaskViewOptions,
    pub(crate) today: NaiveDate,
    pub(crate) csv_path: PathBuf,
    pub(crate) all_tasks: Vec<Task>,
    pub(crate) all_habits: Vec<Task>,
    pub(crate) assignment: crate::tasks::task::AssignmentContext,
    pub(crate) assignment_filter: Option<crate::users::UserId>,
    pub(crate) active_view: Option<View>,
    pub(crate) initial_search: Option<String>,
    pub(crate) agenda_runner: Box<dyn ShellRunner>,
    pub(crate) open_runner: Box<dyn ShellRunner>,
    pub(crate) config: Config,
    pub(crate) agent_kind: AgentKind,
    pub(crate) instance: String,
    pub(crate) db: Db,
    pub(crate) search: crate::picker::App,
    pub(crate) panel_side: PanelSide,
    pub(crate) skip_daily_triage_check: bool,
    pub(crate) server_ingress: crate::server::IngressId,
    pub(crate) server_local_capability: crate::server::lifecycle::LeaseId,
    pub(crate) receiver: crate::tui::receiver::ReceiverRuntime,
}

#[cfg(test)]
fn app_workspace_paths(command_context: &crate::workspace::CommandContext) -> (PathBuf, PathBuf) {
    (
        command_context.workspace.root().to_path_buf(),
        command_context.workspace.paths().state_db(),
    )
}

fn reconcile_triage_startup(
    workspace: &crate::workspace::WorkspaceContext,
    enabled: bool,
    tasks_path: &std::path::Path,
) -> anyhow::Result<(Vec<Task>, Vec<Task>)> {
    crate::tasks::triage_habits::apply_triage_habits_config(workspace, enabled)?;
    let tasks = crate::tasks::task::load_tasks(tasks_path)?;
    let habits = crate::tasks::task::load_habits(&workspace.root().join("tasks/habits.csv"))?;
    Ok((tasks, habits))
}

impl App {
    pub(crate) fn new(init: AppInit) -> Self {
        let AppInit {
            command_context,
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
            agenda_runner,
            open_runner,
            config,
            agent_kind,
            instance,
            db,
            search,
            panel_side,
            skip_daily_triage_check,
            server_ingress,
            server_local_capability,
            receiver,
        } = init;
        // A signal left behind by a run whose shell died must never close a tab
        // opened later, so this shell starts with none pending.
        crate::skill_session::signal::clear_all(&command_context.workspace);
        let configured_skill_sessions =
            crate::env::get_raw(&command_context, crate::skill_session::ENV_VAR);
        let (all_tasks, all_habits) = reconcile_triage_startup(
            &command_context.workspace,
            config.enable_triage_habits,
            &csv_path,
        )
        .unwrap_or_else(|error| {
            crate::logging::log(format!("triage startup reconciliation failed: {error:#}"));
            (all_tasks, all_habits)
        });
        let interactive_actor = command_context.actor.clone();
        let agent_command = crate::agent::configured_command(&command_context, agent_kind);
        let twilio_from = crate::env::get(&command_context, "twilio_from_number");
        let persistent_warning = receiver_phone_warning(&config, twilio_from.as_deref());
        let receiver_sync_runtime: Box<dyn ReceiverSyncRuntime> =
            Box::new(SystemReceiverSyncRuntime);
        let sync_status_next_poll = receiver_sync_runtime.monotonic_now();
        let last_seen_downstream_id = receiver_sync_runtime
            .latest_successful_downstream_id(command_context.workspace.paths());
        let tasks = TasksState::new(TasksStateInit {
            view,
            task_options,
            today,
            active_view,
            all_tasks,
            all_habits,
            assignment,
            assignment_filter,
            initial_search,
            tag_styles: crate::personalization::load_tag_styles(&command_context.workspace),
        });
        let shell = ShellState::new(search, panel_side);
        let context = AppContext::new(AppContextInit {
            command: command_context,
            config,
            agent_kind,
            agent_command,
            csv_path,
            log_path: crate::logging::path(),
            server_ingress,
            server_local_capability,
        });
        let brain = BrainPanelState::new(BrainPanelStateInit {
            instance,
            interactive_actor,
            configured_skill_sessions,
        });
        let services = AppServices::new(AppServicesInit {
            agenda_runner,
            open_runner,
            db,
            receiver_intent_refresher: Box::new(crate::server::control::ServerClient::default()),
            receiver_sync_runtime,
        });
        let status = StatusState::new(StatusStateInit {
            triage_day: today,
            skip_daily_triage_check,
            persistent_warning,
            sync_status_next_poll,
            last_seen_downstream_id,
        });
        Self {
            context,
            tasks,
            brain,
            shell,
            overlay: None,
            services,
            status,
            receiver,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use crate::workspace::{
        CommandContext, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName,
    };

    use super::{app_workspace_paths, reconcile_triage_startup};

    #[test]
    fn app_runtime_paths_come_only_from_the_selected_command_context() {
        let home = Path::new("/home/tester");
        let workspace = WorkspaceContext::new(
            home,
            WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b").expect("valid id"),
            WorkspaceName::parse("family").expect("valid name"),
            Path::new("/workspaces/family"),
            "pablo",
            Path::new("/workspaces"),
        )
        .expect("workspace context");
        let context = CommandContext::for_test(
            Arc::new(workspace),
            RegistryStore::from_path(home.join(".config/brain/env.json")),
            "pablo",
        );

        assert_eq!(
            app_workspace_paths(&context),
            (
                Path::new("/workspaces/family").to_path_buf(),
                home.join(".cache/brain/workspaces/8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b/state.db"),
            )
        );
    }

    #[test]
    fn startup_reconciliation_restores_managed_definitions_before_loading_rows() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("family");
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(root.join(".config")).unwrap();
        let tasks_path = root.join("tasks/tasks.csv");
        std::fs::write(
            &tasks_path,
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tasks/habits.csv"),
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        std::fs::write(root.join(".config/config.json"), "{}\n").unwrap();
        let workspace = WorkspaceContext::new(
            temporary.path(),
            WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
            WorkspaceName::parse("family").unwrap(),
            &root,
            "member",
            temporary.path(),
        )
        .unwrap();

        let (_, habits) = reconcile_triage_startup(&workspace, true, &tasks_path).unwrap();

        assert_eq!(
            habits
                .iter()
                .filter(|habit| habit.is_managed_triage())
                .count(),
            2
        );
    }
}
