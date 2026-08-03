//! Habits web command dispatch.

use anyhow::Result;

pub fn run_habits(
    args: &crate::cli::HabitsArgs,
    context: &crate::workspace::CommandContext,
) -> Result<()> {
    match &args.action {
        None => open_habits(context),
        Some(crate::cli::HabitsAction::Revive(revive)) => {
            crate::tasks::revive::run(context.workspace.root(), &revive.query.join(" "))
        }
        Some(crate::cli::HabitsAction::Skip(skip)) => {
            crate::tasks::skip::run(context.workspace.root(), &skip.id, skip.until.as_deref())
        }
    }
}

fn open_habits(context: &crate::workspace::CommandContext) -> Result<()> {
    let theme = crate::theme::Theme::active();
    eprintln!("{}", crate::server::lifecycle::format_ensure_plan(theme));
    crate::logging::log("habits ensure server");
    let port = crate::server::lifecycle::ensure_running()?;
    let target = crate::server::habits_url(port, context.workspace.id());
    crate::logging::log(format!("habits open {target}"));
    println!("{}", theme.info(&format!("Opening {target}")));
    crate::logging::log(format!("spawn open {target}"));
    let _ = std::process::Command::new("open").arg(&target).spawn();
    Ok(())
}
