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
    let inherited = inherited_identity();
    match users {
        Ok(users) => {
            if let Some(actor) = inherited
                .as_ref()
                .and_then(|(id, channel)| adopt(id, *channel, &users))
            {
                return Ok(actor);
            }
            Ok(resolve_actor(&local, RequestIdentity::Local, &users)?)
        }
        Err(error) if error.is_missing_store() => Ok(ActorContext::new(
            local.clone(),
            local.to_string(),
            Channel::Interactive,
        )),
        Err(error) => Err(error.into()),
    }
}

/// The identity Brain passed to this child process, if it is one.
///
/// `BRAIN_ACTOR_ID` and `BRAIN_CHANNEL` travel to everything Brain launches
/// (see `WorkspaceContext::integration_env`). A command an agent runs inside a
/// panel opened for an inbound message therefore acts as **that sender**, which
/// is what makes a task created in answer to a text belong to the person who
/// sent it. A plain shell has neither variable set and resolves the machine's
/// own person, which is what someone typing `brain` wants.
fn inherited_identity() -> Option<(String, Channel)> {
    let id = std::env::var("BRAIN_ACTOR_ID").ok()?;
    let id = id.trim().to_owned();
    if id.is_empty() {
        return None;
    }
    let channel = std::env::var("BRAIN_CHANNEL")
        .ok()
        .map_or(Channel::Interactive, |value| Channel::parse(&value));
    Some((id, channel))
}

/// Adopt an inherited identity **only** when it names a real portable member.
///
/// An unknown id falls back to the machine's person rather than failing: the
/// variable is a hint from a parent process, not an authorization, and the
/// authorization already happened where the request was authenticated.
fn adopt(id: &str, channel: Channel, users: &crate::users::Users) -> Option<ActorContext> {
    let id = crate::users::UserId::parse(id).ok()?;
    let user = users.user(&id)?;
    Some(ActorContext::new(
        user.id.clone(),
        user.name.clone(),
        channel,
    ))
}

#[cfg(test)]
pub(crate) fn test_actor(user_id: &str) -> ActorContext {
    let user_id = crate::users::UserId::parse(user_id).expect("valid test actor id");
    ActorContext::new(user_id.clone(), user_id.to_string(), Channel::Interactive)
}

#[cfg(test)]
mod tests {
    use super::{Channel, adopt};
    use crate::users::Users;

    fn roster() -> Users {
        Users::parse(
            br#"{"schema_version":1,"users":[
                {"id":"pablo","name":"Pablo"},
                {"id":"wife","name":"Kristi"}
            ]}"#,
        )
        .expect("roster")
    }

    #[test]
    fn an_inherited_identity_that_names_a_member_is_adopted() {
        let actor = adopt("wife", Channel::Sms, &roster()).expect("adopted");
        assert_eq!(actor.user_id().to_string(), "wife");
        assert_eq!(actor.display_name(), "Kristi");
        assert_eq!(actor.channel(), Channel::Sms);
    }

    #[test]
    fn an_inherited_identity_that_names_nobody_is_ignored() {
        // The variable is a hint from a parent process, not an authorization.
        assert!(adopt("stranger", Channel::Sms, &roster()).is_none());
        assert!(adopt("Not An Id", Channel::Sms, &roster()).is_none());
    }

    #[test]
    fn an_unrecognized_channel_never_claims_an_inbound_one() {
        assert_eq!(Channel::parse("sms"), Channel::Sms);
        assert_eq!(Channel::parse("email"), Channel::Email);
        assert_eq!(Channel::parse("interactive"), Channel::Interactive);
        assert_eq!(Channel::parse("nonsense"), Channel::Interactive);
        assert_eq!(Channel::parse(""), Channel::Interactive);
    }
}
