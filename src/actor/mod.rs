//! Effective person identity for one local or authenticated inbound request.

mod context;
mod resolve;

pub use context::{ActorContext, Channel};
pub use resolve::{ActorError, RequestIdentity, resolve_actor};

/// Resolve the selected machine-local person through the portable registry.
pub fn local_actor(workspace: &crate::workspace::WorkspaceContext) -> anyhow::Result<ActorContext> {
    resolve_local_actor(workspace, crate::users::UsersStore::load(workspace))
}

pub(crate) fn local_actor_read_only(
    workspace: &crate::workspace::WorkspaceContext,
) -> anyhow::Result<ActorContext> {
    resolve_local_actor(
        workspace,
        crate::users::UsersStore::load_from(&crate::users::UsersStore::path(workspace)),
    )
}

fn resolve_local_actor(
    workspace: &crate::workspace::WorkspaceContext,
    users: Result<crate::users::Users, crate::users::UsersError>,
) -> anyhow::Result<ActorContext> {
    let local = crate::users::UserId::parse(workspace.local_user_id())?;
    match users {
        Ok(users) => Ok(resolve_actor(&local, RequestIdentity::Local, &users)?),
        Err(error) if error.is_missing_store() => Ok(ActorContext::new(
            local.clone(),
            local.to_string(),
            Channel::Interactive,
        )),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
pub(crate) fn test_actor(user_id: &str) -> ActorContext {
    let user_id = crate::users::UserId::parse(user_id).expect("valid test actor id");
    ActorContext::new(user_id.clone(), user_id.to_string(), Channel::Interactive)
}
