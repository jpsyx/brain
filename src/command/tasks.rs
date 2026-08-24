//! Tasks and TUI command handler.

use std::path::PathBuf;

use anyhow::Result;
use chrono::Local;

use crate::tasks::cli::{Cli as TasksCli, Command as TasksCommand};

pub fn launch(
    mut cli: TasksCli,
    context: &crate::workspace::CommandContext,
    agent_kind: crate::session::AgentKind,
    skip_daily_triage_check: bool,
) -> Result<()> {
    if !matches!(cli.command, Some(TasksCommand::Doctor)) {
        prepare_empty_workspace(context)?;
    }
    let root = context.workspace.root();
    let today = Local::now().date_naive();
    let initial = match cli.command.take() {
        Some(TasksCommand::Complete(args)) => {
            return crate::tasks::complete::run(context, &args.id);
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
                &context.registry_store,
                &context.workspace,
                &context.actor,
                &request,
            )?;
            println!("{}", format_add_result(&result, args.json)?);
            return Ok(());
        }
        Some(TasksCommand::Set(args)) => {
            return set::run(&context.registry_store, &context.workspace, *args);
        }
        Some(TasksCommand::SyncAgenda(args)) => {
            return sync_agenda::run(context, &args);
        }
        Some(TasksCommand::Lint(args)) => return run_lint(context, args.fix),
        Some(TasksCommand::Streak(args)) => return run_streak(context, &args),
        Some(TasksCommand::AgendaPdf(args)) => return sync_agenda::run_pdf(context, &args),
        Some(TasksCommand::Chronic(args)) => return scan::run_chronic(context, &args),
        Some(TasksCommand::StaleWaiting(args)) => return scan::run_waiting(context, &args),
        Some(TasksCommand::Linked(args)) => return scan::run_linked(context, &args),
        Some(TasksCommand::AgendaAppendix(args)) => {
            return sync_agenda::run_appendix(context, &args);
        }
        Some(TasksCommand::Remove(args)) => return mutate::run_remove(context, &args),
        Some(TasksCommand::Defer(args)) => return mutate::run_defer(context, &args),
        Some(TasksCommand::Touch(args)) => return mutate::run_touch(context, &args),
        Some(TasksCommand::Assign(args)) => return mutate::run_assign(context, &args),
        Some(TasksCommand::Search(args)) => browse::Initial::CustomSearch(args.query.join(" ")),
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
        None => browse::resolve_query(&cli.query, today),
    };
    browse::run(
        initial,
        &mut cli,
        context,
        today,
        agent_kind,
        skip_daily_triage_check,
    )
}

/// `brain tasks lint [--fix]`, and the rule half of `brain reindex --tasks`.
pub(crate) fn run_lint(context: &crate::workspace::CommandContext, fix: bool) -> Result<()> {
    let report = lint_report(context, fix)?;
    eprint!(
        "{}",
        crate::tasks::rules::render(&report, fix, crate::theme::Theme::active())
    );
    if !fix && !report.issues.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

/// Run the rules, holding the task-store lock while they may write.
pub(crate) fn lint_report(
    context: &crate::workspace::CommandContext,
    fix: bool,
) -> Result<crate::tasks::rules::LintReport> {
    let _owner = fix
        .then(|| crate::tasks::store_lock::TaskStoreOwner::acquire(&context.workspace))
        .transpose()?;
    crate::tasks::rules::run(context.workspace.root(), Local::now().date_naive(), fix)
}

/// `brain tasks streak <name> [status|mark|unmark]`.
fn run_streak(
    context: &crate::workspace::CommandContext,
    args: &crate::tasks::cli::StreakArgs,
) -> Result<()> {
    use crate::tasks::cli::StreakAction;

    let date = match &args.date {
        Some(raw) => chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!("'{raw}' is not a date (expected YYYY-MM-DD)"))?,
        None => Local::now().date_naive(),
    };
    let root = context.workspace.root();
    let status = match args.action.as_ref().unwrap_or(&StreakAction::Status) {
        StreakAction::Status => crate::tasks::streak::status(root, &args.name, date)?,
        StreakAction::Mark => crate::tasks::streak::mark(root, &args.name, date)?,
        StreakAction::Unmark => crate::tasks::streak::unmark(root, &args.name, date)?,
    };
    println!("{}", serde_json::to_string(&status)?);
    Ok(())
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

mod set;

mod mutate;

mod scan;
pub(crate) use mutate::run_backlog_move;

mod sync_agenda;

mod browse;

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
