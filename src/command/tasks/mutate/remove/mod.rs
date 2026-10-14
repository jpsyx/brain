//! Consent and process reporting around the deterministic removal service.
mod interactive;
mod report;

use anyhow::Result;
use chrono::Local;
use serde_json::{Value, json};
use std::io::{IsTerminal as _, Write as _};

use crate::tasks::cli::{Filters, RemoveArgs};
use crate::tasks::removal::{
    self,
    model::{Options, Problem},
};
use crate::workspace::CommandContext;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode<'a> {
    Targets,
    Resume(&'a str),
    Status(&'a str),
    Abandon(&'a str),
}

/// One pure validation boundary, before any target resolution or operation IO.
pub(super) fn validate_mode<'a>(
    args: &'a RemoveArgs,
    filters: &Filters,
    alternate_csv: bool,
    query: &[String],
) -> std::result::Result<Mode<'a>, Problem> {
    if has_filters(filters) || alternate_csv || !query.is_empty() {
        return Err(Problem::new(
            "removal_filters_not_supported",
            "removal accepts explicit targets only; listing filters, queries and alternate CSV stores cannot select removal targets",
            json!({}),
        ));
    }
    let modes = usize::from(args.resume.is_some())
        + usize::from(args.operation_status.is_some())
        + usize::from(args.abandon.is_some());
    if modes == 0 {
        if args.ids.is_empty() || (args.dry_run && args.expect_plan.is_some()) {
            return Err(invalid_mode());
        }
        return Ok(Mode::Targets);
    }
    if modes != 1
        || !args.ids.is_empty()
        || args.habit
        || args.dependents.is_some()
        || args.linked_issue.is_some()
        || args.dry_run
        || args.expect_plan.is_some()
    {
        return Err(invalid_mode());
    }
    if let Some(id) = args.abandon.as_deref() {
        return if args.yes {
            Ok(Mode::Abandon(id))
        } else {
            Err(invalid_mode())
        };
    }
    if args.yes {
        return Err(invalid_mode());
    }
    args.resume
        .as_deref()
        .map(Mode::Resume)
        .or_else(|| args.operation_status.as_deref().map(Mode::Status))
        .ok_or_else(invalid_mode)
}
fn invalid_mode() -> Problem {
    Problem::new(
        "invalid_removal_mode",
        "resume/status reject consent and target/policy/preview flags; abandon requires --yes and rejects target/policy/preview flags",
        json!({}),
    )
}
fn has_filters(f: &Filters) -> bool {
    f.hard_deadline.is_some()
        || f.status.is_some()
        || f.priority.is_some()
        || f.task_type.is_some()
        || f.project.is_some()
        || f.energy.is_some()
        || f.context.is_some()
        || f.assigned_to.is_some()
        || f.linear_issue.is_some()
        || f.past_due
        || f.mit
        || f.stale
        || f.no_due
        || f.blocked
        || f.include_done
        || f.include_deferred
        || f.search.is_some()
}

pub(super) fn run(
    context: &CommandContext,
    args: &RemoveArgs,
    filters: &Filters,
    alternate_csv: bool,
    query: &[String],
) -> Result<()> {
    let outcome = validate_mode(args, filters, alternate_csv, query).and_then(|mode| match mode {
        Mode::Targets => targets(context, args),
        Mode::Resume(id) => {
            removal::resume(context, id).map(|receipt| (report::result(&receipt), 0))
        }
        Mode::Status(id) => {
            removal::operation_status(context, id).map(|op| (report::operation(&op), 0))
        }
        Mode::Abandon(id) => removal::abandon(context, id).map(|op| (report::operation(&op), 0)),
    });
    let (value, code) =
        outcome.unwrap_or_else(|error| (report::error(&error), report::exit_code(&error)));
    emit(&value, args.json)?;
    if code == 0 {
        Ok(())
    } else {
        Err(crate::command::tasks::RenderedExit(code).into())
    }
}
fn targets(
    context: &CommandContext,
    args: &RemoveArgs,
) -> std::result::Result<(Value, i32), Problem> {
    targets_with_clock(context, args, || Local::now().date_naive())
}
fn targets_with_clock(
    context: &CommandContext,
    args: &RemoveArgs,
    mut today: impl FnMut() -> chrono::NaiveDate,
) -> std::result::Result<(Value, i32), Problem> {
    let mut options = Options {
        habit: args.habit,
        dependents: args.dependents.unwrap_or_default(),
        linked_issue: args.linked_issue.unwrap_or_default(),
    };
    let mut plan = removal::preview(context, &args.ids, options.clone(), today())?;
    if args.dry_run {
        let code = plan.blockers.first().map_or(0, report::exit_code);
        return Ok((report::preview(&plan), code));
    }
    let interactive = !args.json && !args.yes && std::io::stdin().is_terminal();
    if interactive {
        emit(&report::preview(&plan), false).map_err(|_| prompt_failed())?;
        while !plan.blockers.is_empty() {
            if !interactive::choose_policies(&plan, args, &mut options)? {
                return Err(plan.blockers[0].clone());
            }
            plan = removal::preview(context, &args.ids, options.clone(), today())?;
            emit(&report::preview(&plan), false).map_err(|_| prompt_failed())?;
        }
    }
    if let Some(expected) = &args.expect_plan {
        if expected != &plan.fingerprint {
            return Err(Problem::new(
                "stale_plan",
                "the expected fingerprint does not match the fresh removal preview",
                json!({"expected": expected, "actual": plan.fingerprint}),
            ));
        }
    }
    if !args.yes && !interactive {
        return Err(confirmation_required());
    }
    if let Some(blocker) = plan.blockers.first() {
        return Err(blocker.clone());
    }
    if interactive && !interactive::confirm()? {
        return Err(confirmation_required());
    }
    // Read the date again after the human has reviewed/confirmed the exact plan.
    removal::apply(context, &plan, &plan.fingerprint, today())
        .map(|receipt| (report::result(&receipt), 0))
}
fn confirmation_required() -> Problem {
    Problem::new(
        "confirmation_required",
        "review a dry-run preview, then pass --yes to consent to removal",
        json!({}),
    )
}
fn prompt_failed() -> Problem {
    Problem::new(
        "prompt_failed",
        "could not read or write the removal confirmation",
        json!({}),
    )
}
fn emit(value: &Value, json: bool) -> Result<()> {
    if json {
        let mut out = std::io::stdout().lock();
        serde_json::to_writer(&mut out, value)?;
        writeln!(out)?;
    } else {
        std::io::stderr()
            .lock()
            .write_all(report::human(value, crate::theme::Theme::active()).as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
