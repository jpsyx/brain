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
    if !matches!(cli.command, Some(TasksCommand::Doctor)) {
        prepare_empty_workspace(context)?;
    }
    let root = context.workspace.root();
    let today = Local::now().date_naive();
    let initial = match cli.command.take() {
        Some(TasksCommand::Complete(args)) => {
            return crate::tasks::complete::run(&context.workspace, &args.id, &context.actor);
        }
        Some(TasksCommand::Add(args)) => {
            let request = crate::tasks::add::CreateRequest {
                name: args.name,
                task_type: args.task_type,
                priority: args.priority,
                due: args.due,
                start: args.start,
                hard_deadline: args.hard_deadline,
                see_also: args.see_also,
                notes: args.notes,
                project: args.project,
                energy: args.energy,
                context: args.context,
                duration: args.duration,
                blocked_by: args.blocked_by,
                assigned_to: args.assigned_to,
                linear_issue: args.linear_issue,
                habit: args.habit,
                interval: args.interval,
                unit: args.unit,
                ideal_time: args.ideal_time,
                chunks: args.chunks,
            };
            let result = crate::tasks::add::create_in_workspace(
                &context.workspace,
                &context.actor,
                &request,
            )?;
            println!("{}", format_add_result(&result, args.json)?);
            return Ok(());
        }
        Some(TasksCommand::Set(args)) => {
            return run_set(&context.workspace, *args);
        }
        Some(TasksCommand::Search(args)) => Initial::CustomSearch(args.query.join(" ")),
        Some(TasksCommand::Doctor) => {
            let db_path = context.workspace.paths().state_db();
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
            let primary_health_path = crate::agent::primary_session_health_check().map_or_else(
                || root.to_path_buf(),
                |descriptor| descriptor.path(root, &home),
            );
            eprintln!(
                "{}",
                crate::tasks::doctor::format_doctor_plan(
                    &db_path,
                    &primary_health_path,
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
            let compatibility = crate::agent::registrations()
                .iter()
                .filter_map(|registration| {
                    let command = registration.configured_command(context);
                    registration
                        .compatibility(&command)
                        .map(|result| (registration.kind(), result))
                })
                .collect::<Vec<_>>();
            let diagnostic = crate::tasks::doctor::run_doctor_for_workspace(
                &db_path,
                root,
                &home,
                sync_ready,
                &compatibility,
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

fn prepare_empty_workspace(context: &crate::workspace::CommandContext) -> Result<()> {
    if !crate::workspace::is_empty_workspace(context.workspace.root())? {
        return Ok(());
    }

    let sync_config = crate::sync::config::SyncConfig::load(context);
    if sync_config.is_configured() {
        eprintln!(
            "{}",
            crate::theme::Theme::active().info(
                "This workspace is empty; finishing its configured sync before initialization…",
            )
        );
        if !crate::command::sync::run_startup_sync(context, crate::sync::args::Direction::Pull)? {
            anyhow::bail!(
                "workspace initialization stopped because the configured sync did not complete"
            );
        }
    }

    if !crate::workspace::initialize_if_empty(&context.workspace)? {
        return Ok(());
    }
    eprintln!(
        "{}",
        crate::theme::Theme::active().success("Initialized the empty workspace")
    );

    if sync_config.is_configured()
        && !crate::command::sync::run_startup_sync(context, crate::sync::args::Direction::Push)?
    {
        anyhow::bail!(
            "workspace initialization completed locally, but the configured sync push did not complete"
        );
    }
    Ok(())
}

fn format_add_result(result: &crate::tasks::add::CreateResult, json: bool) -> Result<String> {
    if json {
        Ok(serde_json::to_string(result)?)
    } else {
        Ok(result
            .created
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// The editable fields offered by the interactive picker, in prompt order.
/// Each entry is (menu label, the `Edit` slot it fills).
const SET_FIELDS: [(&str, &str); 6] = [
    ("due date", "due"),
    ("priority", "priority"),
    ("title", "name"),
    ("status", "status"),
    ("notes", "notes"),
    ("project", "project"),
];

fn run_set(
    workspace: &crate::workspace::WorkspaceContext,
    args: crate::tasks::cli::SetArgs,
) -> Result<()> {
    let mut edit = crate::tasks::set::Edit {
        name: args.name,
        due: args.due,
        priority: args.priority,
        status: args.status,
        notes: args.notes,
        project: args.project,
        linear_issue: args.linear_issue,
        duration: args.duration,
        ideal_time: args.ideal_time,
        habit: args.habit,
    };
    if edit.is_empty() {
        prompt_for_field(&mut edit, &args.id)?;
    }
    let plan = crate::tasks::set::set_in_workspace(workspace, &args.id, &edit)?;
    if args.json {
        println!("{}", serde_json::to_string(&SetReport::from(&plan))?);
    } else {
        eprint!("{}", format_set_plan(&plan, crate::theme::Theme::active()));
    }
    Ok(())
}

/// Ask a human which field to change and what to change it to.
///
/// Only reached when no field flag was passed at all, so an agent never lands
/// here. With no TTY there is nothing to ask, and the empty edit falls through
/// to `plan`'s "pass at least one of …" error.
fn prompt_for_field(edit: &mut crate::tasks::set::Edit, id: &str) -> Result<()> {
    use std::fmt::Write as _;

    let theme = crate::theme::Theme::active();
    let mut menu = format!("{}\n", theme.heading(&format!("Edit {id}")));
    for (index, (label, _)) in SET_FIELDS.iter().enumerate() {
        let _ = writeln!(
            menu,
            "  {} {}",
            theme.accent(&format!("{}.", index + 1)),
            theme.value(label)
        );
    }
    eprint!("{menu}");
    let Some(answer) = crate::command::configuration::prompt_tty_line(&theme.prompt("field: "))?
    else {
        return Ok(());
    };
    let answer = answer.trim().to_ascii_lowercase();
    let Some((label, slot)) = SET_FIELDS
        .iter()
        .enumerate()
        .find(|(index, (label, slot))| {
            answer == (index + 1).to_string() || answer == *label || answer == *slot
        })
        .map(|(_, entry)| *entry)
    else {
        anyhow::bail!("'{answer}' is not one of the editable fields");
    };
    let Some(value) =
        crate::command::configuration::prompt_tty_line(&theme.prompt(&format!("new {label}: ")))?
    else {
        return Ok(());
    };
    let value = value.trim_end_matches(['\n', '\r']).to_owned();
    match slot {
        "due" => edit.due = Some(value),
        "priority" => edit.priority = Some(value),
        "name" => edit.name = Some(value),
        "status" => edit.status = Some(value),
        "notes" => edit.notes = Some(value),
        _ => edit.project = Some(value),
    }
    Ok(())
}

/// JSON shape for `--json`: the row touched plus one entry per changed column.
#[derive(serde::Serialize)]
struct SetReport<'a> {
    task_id: &'a str,
    task_name: &'a str,
    kind: &'static str,
    changed: Vec<SetReportChange<'a>>,
}

#[derive(serde::Serialize)]
struct SetReportChange<'a> {
    field: &'a str,
    from: &'a str,
    to: &'a str,
}

impl<'a> From<&'a crate::tasks::set::SetPlan> for SetReport<'a> {
    fn from(plan: &'a crate::tasks::set::SetPlan) -> Self {
        Self {
            task_id: &plan.task_id,
            task_name: &plan.task_name,
            kind: if plan.is_habit { "habit" } else { "task" },
            changed: plan
                .changes
                .iter()
                .map(|change| SetReportChange {
                    field: &change.column,
                    from: &change.before,
                    to: &change.after,
                })
                .collect(),
        }
    }
}

fn format_set_plan(plan: &crate::tasks::set::SetPlan, theme: crate::theme::Theme) -> String {
    use std::fmt::Write as _;

    if plan.is_noop() {
        return format!(
            "{} {}  {} {}\n",
            theme.info("unchanged:"),
            theme.accent(&plan.task_id),
            theme.value(&plan.task_name),
            theme.muted("(already had those values)")
        );
    }
    let mut out = format!(
        "{} {}  {}{}\n",
        theme.success("updated:"),
        theme.accent(&plan.task_id),
        theme.value(&plan.task_name),
        if plan.is_habit {
            format!(" {}", theme.muted("(habit)"))
        } else {
            String::new()
        }
    );
    for change in &plan.changes {
        let before = if change.before.trim().is_empty() {
            "(empty)".to_owned()
        } else {
            change.before.clone()
        };
        let after = if change.after.trim().is_empty() {
            "(empty)".to_owned()
        } else {
            change.after.clone()
        };
        let _ = writeln!(
            out,
            "  {} {} {} {}",
            theme.muted(&format!("{}:", change.column)),
            theme.muted(&before),
            theme.muted("→"),
            theme.value(&after)
        );
    }
    out
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
    use super::format_add_result;
    use crate::tasks::add::{CreateResult, CreatedRow};

    #[test]
    fn add_output_is_stable_text_or_json() {
        let result = CreateResult {
            created: vec![CreatedRow {
                id: "T1".to_owned(),
                name: "Reply".to_owned(),
                kind: "task".to_owned(),
            }],
        };
        assert_eq!(format_add_result(&result, false).unwrap(), "T1");
        assert_eq!(
            format_add_result(&result, true).unwrap(),
            r#"{"created":[{"id":"T1","name":"Reply","kind":"task"}]}"#
        );
    }
}
