//! Habits web command dispatch.

use anyhow::Result;

pub fn run_habits(
    args: &crate::cli::HabitsArgs,
    context: &crate::workspace::CommandContext,
) -> Result<()> {
    match &args.action {
        None => open_habits(context),
        Some(crate::cli::HabitsAction::Revive(revive)) => {
            crate::tasks::revive::run(&context.workspace, &revive.query.join(" "), &context.actor)
        }
        Some(crate::cli::HabitsAction::Skip(skip)) => crate::tasks::skip::run(
            &context.workspace,
            &skip.id,
            skip.until.as_deref(),
            &context.actor,
        ),
    }
}

fn open_habits(context: &crate::workspace::CommandContext) -> Result<()> {
    let theme = crate::theme::Theme::active();
    crate::logging::log("habits connect to existing server");
    let port = crate::server::lifecycle::ServerClient::default()
        .connect_existing()?
        .port;
    let target = crate::server::habits_url(port, context.workspace.id());
    crate::logging::log(format!("habits open {target}"));
    println!("{}", theme.info(&format!("Opening {target}")));
    crate::logging::log(format!("spawn open {target}"));
    let _ = std::process::Command::new("open").arg(&target).spawn();
    Ok(())
}
