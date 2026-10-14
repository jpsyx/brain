//! Managed triage adapter for the shared authenticated transaction engine.

use crate::tasks::store_lock::TaskStoreOwner;
use crate::tasks::store_transaction::{self, TargetPolicy};
use crate::workspace::WorkspaceContext;
use anyhow::Result;

pub(super) use crate::tasks::store_transaction::{FileChange, recover_pending};

pub(super) fn replace_group(
    workspace: &WorkspaceContext,
    owner: &TaskStoreOwner,
    changes: &[FileChange],
) -> Result<()> {
    store_transaction::replace_group(workspace, owner, TargetPolicy::ManagedTriage, changes)
}
