//! `brain tasks remove|defer|touch|assign` — the native task mutators.
//!
//! Each one acquires the task-store lock, resolves the day's agenda targets,
//! runs the mutation, and prints a themed report. The mutation itself lives in
//! [`crate::tasks::mutate`]; this is the CLI shell.

mod report;

use anyhow::Result;
use chrono::Local;

use crate::tasks::cli::{AssignArgs, DeferArgs, RemoveArgs, TouchArgs};
use crate::tasks::mutate::{assign, backlog, defer, remove, touch};
use crate::theme::Theme;
use crate::workspace::CommandContext;

/// The lock plus the day's targets, held for one mutation.
struct Session {
    _owner: crate::tasks::store_lock::TaskStoreOwner,
    targets: crate::tasks::agenda::Targets,
    today: chrono::NaiveDate,
}

fn open(context: &CommandContext) -> Result<Session> {
    let today = Local::now().date_naive();
    Ok(Session {
        _owner: crate::tasks::store_lock::TaskStoreOwner::acquire(&context.workspace)?,
        targets: crate::tasks::agenda::resolve_targets(
            &context.registry_store,
            &context.workspace,
            today,
        ),
        today,
    })
}

pub(super) fn run_remove(context: &CommandContext, args: &RemoveArgs) -> Result<()> {
    let session = open(context)?;
    let (result, _) = remove::remove_in_root(
        context.workspace.root(),
        &session.targets,
        &args.id,
        args.habit,
        session.today,
    )?;
    eprint!("{}", report::removed(&result, Theme::active()));
    Ok(())
}

pub(super) fn run_defer(context: &CommandContext, args: &DeferArgs) -> Result<()> {
    let when = defer::When::parse(&args.when)?;
    let session = open(context)?;
    let (result, _) = defer::defer_in_root(
        context.workspace.root(),
        &session.targets,
        &args.id,
        when,
        args.no_count,
        session.today,
    )?;
    eprint!("{}", report::deferred(&result, Theme::active()));
    Ok(())
}

pub(super) fn run_touch(context: &CommandContext, args: &TouchArgs) -> Result<()> {
    let session = open(context)?;
    let (result, _) = touch::touch_in_root(
        context.workspace.root(),
        &session.targets,
        &args.id,
        session.today,
    )?;
    eprint!(
        "{}",
        report::touched(&result, &session.today.to_string(), Theme::active())
    );
    Ok(())
}

pub(super) fn run_assign(context: &CommandContext, args: &AssignArgs) -> Result<()> {
    let session = open(context)?;
    let (result, _) = assign::assign_in_root(
        context.workspace.root(),
        &session.targets,
        &args.id,
        &args.user,
        session.today,
    )?;
    eprint!("{}", report::assigned(&result, Theme::active()));
    Ok(())
}

/// `brain backlog park|restore <id>`.
pub(crate) fn run_backlog_move(context: &CommandContext, id: &str, restore: bool) -> Result<()> {
    let session = open(context)?;
    let (result, _) = backlog::backlog_in_root(
        context.workspace.root(),
        &session.targets,
        id,
        restore,
        session.today,
    )?;
    eprint!("{}", report::parked(&result, Theme::active()));
    Ok(())
}
