//! Effective person identity for one local or authenticated inbound request.

mod context;
mod resolve;

pub use context::{ActorContext, Channel};
pub use resolve::{ActorError, RequestIdentity, resolve_actor};

/// Resolve the selected machine-local person through the portable registry.
pub fn local_actor(workspace: &crate::workspace::WorkspaceContext) -> anyhow::Result<ActorContext> {
    let users = crate::users::UsersStore::load(workspace)?;
    let local = crate::users::UserId::parse(workspace.local_user_id())?;
    Ok(resolve_actor(&local, RequestIdentity::Local, &users)?)
}
