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
        active_view: Option<View>,
        initial_search: Option<String>,
        agenda_runner: Box<dyn ShellRunner>,
        open_runner: Box<dyn ShellRunner>,
        config: Config,
        agent_kind: AgentKind,
        interactive_actor: crate::actor::ActorContext,
        instance: String,
        db: Db,
        search: crate::picker::App,
        panel_side: PanelSide,
        skip_daily_triage_check: bool,
    ) -> Self {
        let (brain_root, db_path) = app_workspace_paths(&command_context);
        let query = initial_search.unwrap_or_default();
        let in_search = !query.is_empty();
        let twilio_from = crate::env::get(&command_context, "twilio_from_number");
        let persistent_warning = receiver_phone_warning(&config, twilio_from.as_deref());
        let mut app = Self {
            tag_styles: crate::personalization::load_tag_styles(&command_context.workspace),
            command_context,
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
            scroll: 0,
            last_inner_height: 1,
            last_content_rows: 1,
            brain: None,
            brain_turn_active: false,
            focus: Panel::Tasks,
            triage_brain: None,
            active_brain_tab: BrainTab::Main,
            triage_token: None,
            brain_rect: None,
            instance,
            interactive_actor,
            session_actor: None,
            brain_root,
            db_path,
            log_path: crate::logging::path(),
            alert: None,
            pending_brain_submit: 0,
            palette: None,
            brain_input: None,
            confirm: None,
            link_picker: None,
            help: None,
            flash: None,
            persistent_warning,
            agenda_runner,
            open_runner,
            db,
            receiver_server: None,
            receiver_control: None,
            receiver_rx: None,
            receiver_queue: Vec::new(),
            requested_receiver_actor: None,
            receiver_lease: None,
            receiver_generation: 0,
            receiver_sender: None,
            receiver_recipients: Vec::new(),
            receiver_session_id: None,
            interactive_session_id: None,
            receiver_resume_session: None,
            receiver_started: None,
            receiver_delay_sent: false,
            receiver_retry_at: None,
            receiver_sync_gate: None,
            sync_status: None,
            sync_status_next_poll: Instant::now(),
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

    use super::app_workspace_paths;

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
        let context = CommandContext {
            workspace: Arc::new(workspace),
            registry_store: RegistryStore::from_path(home.join(".config/brain/env.json")),
        };

        assert_eq!(
            app_workspace_paths(&context),
            (
                Path::new("/workspaces/family").to_path_buf(),
                home.join(".cache/brain/workspaces/8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b/state.db"),
            )
        );
    }
}
