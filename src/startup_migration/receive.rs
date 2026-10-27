//! A pending receive permits only its explicit pull or repair continuation before publication.
use crate::{
    cli::{Cli, Cmd, SyncAction},
    sync::{
        args::Direction,
        journal::{Journal, ReceivePhase},
        verify::Outcome,
    },
    workspace::{BootstrapContext, WorkspacePaths},
};
use anyhow::{Result, ensure};

pub(super) fn recover_pull(cli: &mut Cli) -> Result<bool> {
    let Some(Cmd::Sync(args)) = &cli.command else {
        return Ok(false);
    };
    let direction = match (&args.action, args.pull, args.push) {
        (None, true, false) => Direction::Pull,
        (Some(SyncAction::Repair | SyncAction::Init), false, false) => Direction::Resync,
        _ => return Ok(false),
    };
    let Some(record) = crate::workspace::recovery_record(cli)? else {
        return Ok(false);
    };
    let home = super::home_dir()?;
    let paths = WorkspacePaths::new(&home, record.workspace_id);
    if Journal::receive_phase(&paths.sync_journal(), &record.workspace_id.to_string())?
        != Some(ReceivePhase::Receiving)
    {
        return Ok(false);
    }
    super::run_current_with_setup(|| {
        let BootstrapContext::Ready(command) = crate::workspace::bootstrap(cli)? else {
            anyhow::bail!("interrupted receive recovery requires a ready registered workspace");
        };
        ensure!(
            command.workspace.id() == record.workspace_id,
            "receive workspace identity changed during startup"
        );
        let _sync = crate::sync::lock::try_acquire(&paths.sync_lock()).ok_or_else(|| {
            anyhow::anyhow!(
                "workspace sync is busy; retry the requested pull or repair when it completes"
            )
        })?;
        ensure!(
            Journal::receive_phase(&paths.sync_journal(), &record.workspace_id.to_string())?
                == Some(ReceivePhase::Receiving),
            "receive state changed before recovery"
        );
        crate::migration::require_no_active_rollout(&paths)?;
        crate::tasks::schema::ensure_schema_document(command.workspace.root())?;
        transfer(
            &command.workspace,
            &crate::sync::config::SyncConfig::load(&command),
            direction,
            false,
        )
    })?;
    eprintln!(
        "{}",
        crate::theme::Theme::active()
            .success("Workspace receive and package reconciliation complete.")
    );
    Ok(true)
}

/// Continue a receive under the caller's validated UUID sync ownership.
pub(super) fn transfer(
    workspace: &crate::workspace::WorkspaceContext,
    config: &crate::sync::config::SyncConfig,
    direction: Direction,
    installer: bool,
) -> Result<()> {
    let now = chrono::Utc::now();
    let timestamp = now.to_rfc3339();
    let date = now.format("%Y-%m-%d").to_string();
    eprintln!(
        "{}",
        crate::theme::Theme::active().info("Completing the interrupted workspace receive…")
    );
    let outcome = if installer {
        crate::sync::command::recover_installer_receive(
            workspace,
            config,
            (&timestamp, &timestamp, &date),
        )?
    } else {
        crate::sync::command::sync_once(
            workspace,
            config,
            direction,
            (&timestamp, &timestamp, &date),
        )?
    };
    match outcome {
        Outcome::Clean => {
            if direction == Direction::Resync {
                crate::command::sync::reconcile_after_successful_repair(workspace, true)?;
            }
            Ok(())
        }
        Outcome::NeedsAttention(message) | Outcome::Aborted(message) => {
            anyhow::bail!("receive recovery did not complete cleanly: {message}")
        }
    }
}
