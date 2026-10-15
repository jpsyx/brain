//! Consume explicit setup once before strict managed-skill compatibility.
use crate::cli::{Cli, Cmd, WorkspaceAction};
use anyhow::Result;

#[must_use]
pub fn is_requested(cli: &Cli) -> bool {
    matches!(&cli.command, Some(Cmd::Workspace(args)) if matches!(args.action, WorkspaceAction::Create { .. } | WorkspaceAction::Attach { .. } | WorkspaceAction::Repair { .. } | WorkspaceAction::Migrate { .. }))
}

/// Complete one validated setup request and mandatory startup before reporting success.
pub fn run(cli: &mut Cli) -> Result<()> {
    anyhow::ensure!(
        is_requested(cli),
        "expected an explicit workspace setup request"
    );
    super::bootstrap::prepare_setup_request(cli).map_err(super::command::render_error)?;
    let _logging = crate::logging::init(cli.verbose, false)?;
    let mut applied = None;
    let startup = crate::startup_migration::run_current_with_setup(|| {
        if matches!(&cli.command, Some(Cmd::Workspace(args)) if matches!(args.action, WorkspaceAction::Migrate { .. }))
        {
            let explicit_workspace = cli.workspace_selector.is_some();
            let super::BootstrapContext::Ready(context) = super::bootstrap(cli)? else {
                anyhow::bail!("expected migration-only workspace bootstrap");
            };
            let Some(Cmd::Workspace(args)) = &cli.command else {
                unreachable!("validated workspace migration");
            };
            super::command::run_ready(args, &context, explicit_workspace)?;
            applied = Some(Applied::Migration);
        } else {
            let store = super::bootstrap::prepared_setup_store(cli)?;
            let Some(Cmd::Workspace(args)) = &cli.command else {
                unreachable!("validated workspace setup");
            };
            applied = Some(Applied::Registry(super::command::apply_prepared_setup(
                args,
                cli.workspace_selector.as_deref(),
                &store,
            )?));
        }
        Ok(())
    });
    if applied.is_some() {
        startup.map_err(|error| {
            anyhow::anyhow!(
                "workspace setup committed; mandatory startup did not complete: {error}"
            )
        })?;
    } else {
        startup.map_err(super::command::render_error)?;
    }
    if let Applied::Registry(message) = applied.expect("successful startup consumes prepared setup")
    {
        println!("{message}");
    }
    Ok(())
}

enum Applied {
    Registry(String),
    Migration,
}
