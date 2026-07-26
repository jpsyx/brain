//! `App::new`: assemble the initial shell state from the parsed CLI, loaded
//! task/habit lists, injected runners, and the session DB, then build the
//! first body.

use crate::tasks::render::header_lines;
use crate::tasks::view::ViewSpec;
use crate::tui::*;

impl<'a> App<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
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
        instance: String,
        brain_root: PathBuf,
        db_path: PathBuf,
        db: Db,
        search: crate::picker::App,
        panel_side: PanelSide,
    ) -> Self {
        let query = initial_search.unwrap_or_default();
        let in_search = !query.is_empty();
        let mut app = Self {
            today,
            // Seeded to the startup date; `run_tui` overwrites it with the
            // current logical day right after the startup triage check so the
            // first same-day refresh doesn't re-fire the nudge.
            triage_day: today,
            config,
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
            focus: Panel::Tasks,
            brain_rect: None,
            instance,
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
            agenda_runner,
            open_runner,
            db,
            main_view: MainView::Tasks,
            search,
            panel_side,
        };
        app.rebuild_body();
        app
    }
}
