//! Failure-atomic persistence for one selected receiver setup.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use super::SetupPlan;
use crate::command::server::receiver::hooks;
use crate::workspace::CommandContext;

mod lock;
mod snapshot;

use lock::SetupTransactionLock;
use snapshot::SetupSnapshot;

#[cfg(test)]
use snapshot::resolve_symlink_chain;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitStep {
    Providers,
    Users,
    Directory(&'static str),
    Lock(&'static str),
    Hook(&'static str),
}

pub(super) fn persist_plan(plan: &SetupPlan, context: &CommandContext) -> Result<()> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")?;
    persist_plan_with_hook(plan, context, &home, |_| Ok(()))
}

fn persist_plan_with_hook(
    plan: &SetupPlan,
    context: &CommandContext,
    home: &Path,
    mut after_write: impl FnMut(CommitStep) -> Result<()>,
) -> Result<()> {
    let _transaction = SetupTransactionLock::acquire(context.workspace.root())?;
    let mut snapshot = SetupSnapshot::capture(context, home)?;
    let result = (|| {
        crate::env::set_many(context, &plan.providers)?;
        snapshot.record_providers(&plan.providers);
        after_write(CommitStep::Providers)?;
        crate::users::UsersStore::save(&context.workspace, &plan.users)?;
        snapshot.record_file(&crate::users::UsersStore::path(&context.workspace))?;
        after_write(CommitStep::Users)?;
        hooks::install_for_home_with(context.workspace.root(), home, |step| {
            let commit_step = match step {
                hooks::LifecycleInstallStep::Directory(installation) => {
                    CommitStep::Directory(installation.id())
                }
                hooks::LifecycleInstallStep::Lock(installation) => {
                    CommitStep::Lock(installation.id())
                }
                hooks::LifecycleInstallStep::Artifact(installation) => {
                    snapshot.record_hook_step(context.workspace.root(), home, installation)?;
                    CommitStep::Hook(installation.id())
                }
            };
            after_write(commit_step)
        })?;
        Ok(())
    })();
    if let Err(error) = result {
        return match snapshot.restore(context) {
            Ok(()) => Err(error),
            Err(rollback) => {
                Err(error.context(format!("receiver setup rollback also failed: {rollback:#}")))
            }
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests;
