//! Repair machine skill declarations at the existing one-shot setup boundary.
use crate::cli::{Cli, Cmd, EnvAction};
use anyhow::{Result, ensure};

#[must_use]
pub fn is_requested(cli: &Cli) -> bool {
    let Some(Cmd::Env(args)) = &cli.command else {
        return false;
    };
    let Some(EnvAction::Set {
        assignment: Some(assignment),
    }) = &args.action
    else {
        return false;
    };
    let name = assignment
        .split_once('=')
        .map_or(assignment.as_str(), |(name, _)| name);
    matches!(
        crate::settings::normalize_name(name).as_str(),
        "agent_capabilities" | "agent_capabilities.skills"
    )
}

pub fn run(cli: &mut Cli) -> Result<()> {
    ensure!(
        is_requested(cli),
        "expected a machine skill declaration repair"
    );
    let _logging = crate::logging::init(cli.verbose, false)?;
    let mut committed = false;
    let result = crate::startup_migration::run_current_with_setup(|| {
        let crate::workspace::BootstrapContext::Ready(context) = crate::workspace::bootstrap(cli)?
        else {
            anyhow::bail!("expected a selected workspace for machine skill repair");
        };
        let Some(Cmd::Env(args)) = &cli.command else {
            unreachable!("validated env repair")
        };
        crate::command::configuration::run_env(args, &context)?;
        committed = true;
        Ok(())
    });
    result.map_err(|error| if committed {
        anyhow::anyhow!("machine skill declaration repair committed; mandatory startup did not complete: {error}")
    } else { error })
}
