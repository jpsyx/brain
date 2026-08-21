use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::NaiveDate;

use crate::tasks::cli::Cli as TasksCli;
use crate::tasks::selector::{Selector, parse_selector};
use crate::tasks::view::{TaskViewOptions, View};

pub(super) enum Initial {
    View(View),
    Custom(Selector),
    CustomSearch(String),
}

pub(super) fn resolve_query(tokens: &[String], today: NaiveDate) -> Initial {
    if tokens.is_empty() {
        return Initial::View(View::Today);
    }
    if tokens.len() == 1 {
        if let Some(view) = View::from_token(&tokens[0]) {
            return Initial::View(view);
        }
        if let Ok(selector) = parse_selector(Some(&tokens[0]), today) {
            return Initial::Custom(selector);
        }
    }
    Initial::CustomSearch(tokens.join(" "))
}

pub(super) fn run(
    initial: Initial,
    cli: &mut TasksCli,
    context: &crate::workspace::CommandContext,
    today: NaiveDate,
    agent_kind: crate::session::AgentKind,
    skip_daily_triage_check: bool,
) -> Result<()> {
    crate::logging::log("tasks browse");
    let root = context.workspace.root();
    let csv_path = cli.csv.clone().unwrap_or_else(|| default_csv_path(root));
    crate::logging::log(format!("tasks csv {}", csv_path.display()));
    let all_tasks = crate::tasks::task::load_tasks(&csv_path)?;
    crate::logging::log(format!("loaded {} tasks", all_tasks.len()));
    let habits_path = csv_path.with_file_name("habits.csv");
    crate::logging::log(format!("habits csv {}", habits_path.display()));
    let habits = crate::tasks::task::load_habits(&habits_path).unwrap_or_default();
    crate::logging::log(format!("loaded {} habits", habits.len()));

    let (selector, start_view, initial_search) = match initial {
        Initial::View(view) => (view.selector(today), Some(view), None),
        Initial::Custom(selector) => (selector, None, None),
        Initial::CustomSearch(query) => (Selector::All, Some(View::All), Some(query)),
    };
    if cli.display.no_tui
        && let Some(query) = &initial_search
    {
        cli.filters.search = Some(query.clone());
    }
    let initial_data = if start_view == Some(View::Habits) {
        habits.clone()
    } else {
        all_tasks.clone()
    };
    crate::logging::log(format!(
        "build tasks view start_view={start_view:?} initial_rows={} no_tui={}",
        initial_data.len(),
        cli.display.no_tui,
    ));
    let task_options = TaskViewOptions::from(&*cli);
    let mut view =
        crate::tasks::view::build_view(&task_options, &selector, start_view, initial_data, today);
    if cli.display.no_tui {
        if cli.filters.assigned_to.is_some() {
            let assignment = crate::tasks::task::assignment_context_for_workspace(
                &context.workspace,
                &context.actor,
            )?;
            crate::tasks::task::assignment_filter_for_startup(
                &assignment,
                cli.filters.assigned_to.as_deref(),
            )?;
        }
        crate::tasks::view::apply_assignment_filter(&mut view, cli.filters.assigned_to.as_deref());
    }
    crate::logging::log(format!(
        "built tasks view title={:?} shown={} total={}",
        view.title,
        view.tasks.len(),
        view.total,
    ));
    if cli.display.no_tui {
        crate::logging::log("render tasks no-tui");
        let tag_styles = crate::personalization::load_tag_styles(&context.workspace);
        crate::tasks::plain::print_plain(&view, today, cli.display.full_notes, &tag_styles);
    } else {
        crate::logging::set_stdout_enabled(false);
        crate::logging::log("enter tui");
        crate::tui::run_tui(crate::tui::TuiLaunch {
            command_context: context.clone(),
            view,
            task_options,
            agent_kind,
            today,
            csv_path,
            all_tasks,
            all_habits: habits,
            active_view: start_view,
            initial_search,
            skip_daily_triage_check,
        })?;
    }
    Ok(())
}

fn default_csv_path(root: &Path) -> PathBuf {
    root.join("tasks").join("tasks.csv")
}
