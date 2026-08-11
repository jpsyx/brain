//! `App::new`: assemble the initial shell state from the parsed CLI, loaded
//! task/habit lists, injected runners, and the session DB, then build the
//! first body.

use crate::tasks::render::header_lines;
use crate::tasks::view::ViewSpec;
use crate::tui::*;

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

impl<'a> App<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        command_context: crate::workspace::CommandContext,
        view: &ViewSpec,
        cli: &'a Cli,
        today: NaiveDate,
        csv_path: PathBuf,
        all_tasks: Vec<Task>,
        all_habits: Vec<Task>,
        assignment: crate::tasks::task::AssignmentContext,
        assignment_filter: Option<crate::users::UserId>,
        active_view: Option<View>,
        initial_search: Option<String>,
        agenda_runner: Box<dyn ShellRunner>,
        open_runner: Box<dyn ShellRunner>,
        config: Config,
        agent_kind: AgentKind,
        instance: String,
        db: Db,
        search: crate::picker::App,
        panel_side: PanelSide,
        skip_daily_triage_check: bool,
        server_ingress: crate::server::IngressId,
        server_local_capability: crate::server::lifecycle::LeaseId,
    ) -> Self {
        let (brain_root, db_path) = app_workspace_paths(&command_context);
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
        let receiver_enabled = crate::command::server::receiver_enabled(&command_context)
            .unwrap_or_else(|error| {
                crate::logging::log(format!("receiver intent load failed: {error:#}"));
                false
            });
        let query = initial_search.unwrap_or_default();
        let in_search = !query.is_empty();
        let twilio_from = crate::env::get(&command_context, "twilio_from_number");
        let persistent_warning = receiver_phone_warning(&config, twilio_from.as_deref());
        let receiver_sync_runtime: Box<dyn ReceiverSyncRuntime> =
            Box::new(SystemReceiverSyncRuntime);
        let sync_status_next_poll = receiver_sync_runtime.monotonic_now();
        let mut app = Self {
            tag_styles: crate::personalization::load_tag_styles(&command_context.workspace),
            command_context,
            server_ingress,
            server_local_capability,
            today,
            // Seeded to the startup date; `run_tui` overwrites it with the
            // current logical day right after the startup triage check so the
            // first same-day refresh doesn't re-fire the nudge.
            triage_day: today,
            // Armed by `run_tui` only when a startup sync is pending; otherwise
            // the triage check runs immediately and this stays None.
            triage_gate: None,
            skip_daily_triage_check,
            config,
            agent_kind,
            agent_command,
            full_notes: cli.display.full_notes,
            expanded_notes: HashSet::new(),
            cli,
            csv_path,
            all_tasks,
            all_habits,
            active_view,
            header: header_lines(view, cli, active_view),
            body_lines: Vec::new(),
            visual_row_offsets: vec![0],
            visible_tasks: Vec::new(),
            task_line_ranges: Vec::new(),
            selected_task: None,
            pending_count: None,
            base_tasks: view.tasks.clone(),
            query,
            in_search,
            matcher: SkimMatcherV2::default().ignore_case(),
            assignment,
            assignment_filter,
            scroll: 0,
            last_inner_height: 1,
            last_content_rows: 1,
            brain: None,
            #[cfg(test)]
            brain_transport_override: None,
            brain_turn_active: false,
            focus: Panel::Tasks,
            skill_sessions: Vec::new(),
            next_session_tab_id: 0,
            configured_skill_sessions,
            active_brain_tab: BrainTab::Main,
            #[cfg(test)]
            session_done_url_override: None,
            #[cfg(test)]
            session_transport_override: None,
            brain_rect: None,
            instance,
            interactive_actor,
            session_actor: None,
            brain_root,
            db_path,
            log_path: crate::logging::path(),
            alert: None,
            palette: None,
            brain_input: None,
            confirm: None,
            link_picker: None,
            assignee_filter: None,
            help: None,
            sync_log: None,
            flash: None,
            persistent_warning,
            agenda_runner,
            open_runner,
            db,
            receiver_control: None,
            receiver_enabled,
            receiver_intent_refresher: Box::new(crate::server::control::ServerClient::default()),
            receiver_queue: Vec::new(),
            requested_receiver_actor: None,
            receiver_lease: None,
            receiver_generation: 0,
            receiver_sender: None,
            receiver_recipients: Vec::new(),
            receiver_response_email: None,
            receiver_email_reply: None,
            receiver_session_id: None,
            interactive_session_id: None,
            interactive_agent_session_id: None,
            receiver_resume_session: None,
            receiver_started: None,
            receiver_delay_sent: false,
            receiver_retry_at: None,
            receiver_sync_runtime,
            receiver_sync_gate: None,
            sync_status: None,
            sync_status_next_poll,
            main_view: MainView::Tasks,
            logs_view: None,
            search,
            panel_side,
        };
        app.rebuild_body();
        app
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
