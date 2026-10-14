//! Lock-scoped binding to the one UUID-scoped workspace state DB.
use super::install_transaction::{InstallReceipt, StampStore};
use crate::{
    state::Db,
    workspace::{WorkspaceContext, WorkspaceId, WorkspacePaths},
};
use anyhow::Result;
use std::path::PathBuf;

pub(crate) struct WorkspaceStamp {
    path: PathBuf,
    workspace_id: String,
    actor_id: String,
}
impl WorkspaceStamp {
    pub(crate) fn registered(paths: &WorkspacePaths, id: WorkspaceId, actor: &str) -> Self {
        Self {
            path: paths.state_db(),
            workspace_id: id.to_string(),
            actor_id: actor.into(),
        }
    }
    pub(crate) fn selected(workspace: &WorkspaceContext) -> Self {
        Self::registered(workspace.paths(), workspace.id(), workspace.local_user_id())
    }
    pub(crate) fn invalidate(&self) -> Result<()> {
        if self.path.exists() {
            self.open()?.invalidate_skills_receipt()?;
        }
        Ok(())
    }
    fn open(&self) -> Result<Db> {
        Db::open_path_with_legacy_identity(&self.path, &self.workspace_id, &self.actor_id)
    }
    pub(crate) fn read(&self) -> Result<Option<InstallReceipt>> {
        self.open()?.skills_install_receipt()
    }
}
impl StampStore for WorkspaceStamp {
    fn write(&mut self, receipt: &InstallReceipt) -> Result<()> {
        self.open()?.set_skills_install_receipt(receipt)
    }
}
