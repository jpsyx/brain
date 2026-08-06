//! Tasks and TUI command handler.

use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{Local, NaiveDate};

use crate::tasks::cli::{Cli as TasksCli, Command as TasksCommand};
use crate::tasks::selector::{Selector, parse_selector};
use crate::tasks::view::View;

pub fn launch(
    mut cli: TasksCli,
    context: &crate::workspace::CommandContext,
    agent_kind: crate::session::AgentKind,
    with_receiver: bool,
    skip_daily_triage_check: bool,
) -> Result<()> {
    let root = context.workspace.root();
    let today = Local::now().date_naive();
    let initial = match cli.command.take() {
        Some(TasksCommand::Complete(args)) => {
            return crate::tasks::complete::run(&context.workspace, &args.id, &context.actor);
        }
        Some(TasksCommand::Search(args)) => Initial::CustomSearch(args.query.join(" ")),
        Some(TasksCommand::Doctor) => {
            let db_path = context.workspace.paths().state_db();
            let settings_dir = root.join(".claude");
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
            let codex_hooks = home.join(".codex/hooks.json");
            eprintln!(
                "{}",
                crate::tasks::doctor::format_doctor_plan(
                    &db_path,
                    &settings_dir.join("settings.json"),
                    crate::theme::Theme::active(),
                )
            );
            let requirements = crate::workspace::requirements(context)?;
            let sync_ready = requirements.iter().any(|requirement| {
                requirement.scope() == &crate::workspace::RequirementScope::CloudSync
                    && matches!(
                        requirement.status(),
                        crate::workspace::RequirementStatus::Feature(
                            crate::workspace::FeatureStatus::Ready
                        )
                    )
            });
            let diagnostic = crate::tasks::doctor::run_doctor_with_frontends(
                &db_path,
                &settings_dir,
                &codex_hooks,
                sync_ready,
            );
            std::process::exit(crate::tasks::doctor::print_workspace_report(
                &diagnostic,
                context.workspace.name(),
                context.workspace.root(),
                &requirements,
            ));
        }
        None => resolve_query(&cli.query, today),
    };
    browse(
        initial,
        &mut cli,
        context,
        today,
        agent_kind,
        with_receiver,
        skip_daily_triage_check,
    )
}

enum Initial {
    View(View),
    Custom(Selector),
    CustomSearch(String),
}

fn resolve_query(tokens: &[String], today: NaiveDate) -> Initial {
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

fn browse(
    initial: Initial,
    cli: &mut TasksCli,
    context: &crate::workspace::CommandContext,
    today: NaiveDate,
    agent_kind: crate::session::AgentKind,
    with_receiver: bool,
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
    let mut view = crate::tasks::view::build_view(cli, &selector, start_view, initial_data, today);
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
        crate::tui::run_tui(
            context,
            &view,
            cli,
            agent_kind,
            today,
            csv_path,
            all_tasks,
            habits,
            start_view,
            initial_search,
            with_receiver,
            skip_daily_triage_check,
        )?;
    }
    Ok(())
}

fn default_csv_path(root: &Path) -> PathBuf {
    root.join("tasks").join("tasks.csv")
}

pub fn take_codex_flag(args: &mut Vec<String>) -> bool {
    let before = args.len();
    args.retain(|arg| arg != "--codex" && arg != "-cx");
    args.len() != before
}

#[must_use]
pub fn rewrite_mark_grammar(args: Vec<String>) -> Vec<String> {
    if args.len() < 3 || !args[1].eq_ignore_ascii_case("mark") {
        return args;
    }
    let id_position = 2;
    let after_id = &args[id_position + 1..];
    let consume = match after_id {
        [first, second, ..]
            if first.eq_ignore_ascii_case("as") && second.eq_ignore_ascii_case("done") =>
        {
            2
        }
        [first, ..] if first.eq_ignore_ascii_case("done") => 1,
        [] => 0,
        [first, ..] if first.starts_with('-') => 0,
        _ => return args,
    };
    let mut rewritten = Vec::with_capacity(args.len());
    rewritten.push(args[0].clone());
    rewritten.push("complete".to_owned());
    rewritten.push(args[id_position].clone());
    rewritten.extend_from_slice(&args[id_position + 1 + consume..]);
    rewritten
}

#[cfg(test)]
mod tests {
    use super::take_codex_flag;

    #[test]
    fn delegated_codex_flag_is_removed() {
        let mut args = vec!["today".to_owned(), "--codex".to_owned(), "--mit".to_owned()];
        assert!(take_codex_flag(&mut args));
        assert_eq!(args, vec!["today", "--mit"]);
    }

    #[test]
    fn delegated_cx_alias_is_removed() {
        let mut args = vec!["today".to_owned(), "-cx".to_owned(), "--mit".to_owned()];
        assert!(take_codex_flag(&mut args));
        assert_eq!(args, vec!["today", "--mit"]);
    }

    #[test]
    fn absent_delegated_agent_flag_leaves_arguments_unchanged() {
        let mut args = vec!["today".to_owned()];
        assert!(!take_codex_flag(&mut args));
        assert_eq!(args, vec!["today"]);
    }
}
