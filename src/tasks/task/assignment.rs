//! Pure assignment defaults, reassignment validation, and UI visibility.

use anyhow::{Result, anyhow};

use crate::actor::ActorContext;
use crate::users::{UserId, Users};

/// Which assignment surfaces are useful for the selected workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssignmentUiMode {
    pub show_in_detail: bool,
    pub show_create_control: bool,
    pub show_reassign_control: bool,
    pub show_filter: bool,
}

/// One portable member available to assignment controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentUser {
    /// Stable portable person ID persisted in task and habit rows.
    pub id: UserId,
    /// Human-facing workspace display name.
    pub name: String,
}

/// Immutable assignment state derived once for a tasks-shell run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentContext {
    mode: AssignmentUiMode,
    actor_id: UserId,
    users: Vec<AssignmentUser>,
}

impl AssignmentContext {
    /// Build assignment state from the selected workspace's portable users.
    #[must_use]
    pub fn from_users(users: &Users, actor: &ActorContext) -> Self {
        Self {
            mode: assignment_ui_mode(users),
            actor_id: actor.user_id().clone(),
            users: users
                .users
                .iter()
                .map(|user| AssignmentUser {
                    id: user.id.clone(),
                    name: user.name.clone(),
                })
                .collect(),
        }
    }

    /// Build the read-only legacy fallback when no portable registry exists.
    #[must_use]
    pub fn legacy(actor: &ActorContext) -> Self {
        Self {
            mode: AssignmentUiMode {
                show_in_detail: false,
                show_create_control: false,
                show_reassign_control: false,
                show_filter: false,
            },
            actor_id: actor.user_id().clone(),
            users: vec![AssignmentUser {
                id: actor.user_id().clone(),
                name: actor.display_name().to_owned(),
            }],
        }
    }

    /// Assignment surface visibility for this shell.
    #[must_use]
    pub const fn mode(&self) -> AssignmentUiMode {
        self.mode
    }

    /// Immutable actor used as the creation default.
    #[must_use]
    pub const fn actor_id(&self) -> &UserId {
        &self.actor_id
    }

    /// Portable members available to filters and explicit assignment actions.
    #[must_use]
    pub fn users(&self) -> &[AssignmentUser] {
        &self.users
    }
}

/// Load assignment state for the selected workspace, retaining the legacy
/// one-actor fallback only when the portable registry is absent.
///
/// # Errors
///
/// Returns an error when an existing portable registry cannot be loaded.
pub fn assignment_context_for_workspace(
    workspace: &crate::workspace::WorkspaceContext,
    actor: &ActorContext,
) -> Result<AssignmentContext> {
    match crate::users::UsersStore::load(workspace) {
        Ok(users) => Ok(AssignmentContext::from_users(&users, actor)),
        Err(error) if error.is_missing_store() => Ok(AssignmentContext::legacy(actor)),
        Err(error) => Err(error.into()),
    }
}

/// Resolve the startup CLI filter against the selected workspace members.
///
/// # Errors
///
/// Returns an error when the requested ID is invalid or is not a portable
/// member of the selected workspace.
pub fn assignment_filter_for_startup(
    context: &AssignmentContext,
    requested: Option<&str>,
) -> Result<Option<UserId>> {
    let Some(requested) = requested else {
        return Ok(None);
    };
    let requested = UserId::parse(requested)?;
    if !context.users.iter().any(|user| user.id == requested) {
        return Err(anyhow!(
            "--assigned-to must name a selected workspace member; unknown portable user {requested}"
        ));
    }
    Ok(Some(requested))
}

/// Default a new row to the request's immutable effective actor.
#[must_use]
pub fn assignment_for_create(actor: &ActorContext, users: &Users) -> UserId {
    debug_assert!(users.user(actor.user_id()).is_some());
    actor.user_id().clone()
}

/// Preserve the existing assignment unless an explicit portable member is named.
///
/// # Errors
///
/// Returns an error when the requested ID is invalid or not a workspace member.
pub fn assignment_after_edit(
    current: &UserId,
    requested: Option<&str>,
    users: &Users,
) -> Result<UserId> {
    let Some(requested) = requested else {
        return Ok(current.clone());
    };
    let requested = UserId::parse(requested)?;
    if users.user(&requested).is_none() {
        return Err(anyhow!("unknown portable user {requested}"));
    }
    Ok(requested)
}

/// Hide assignment UI for one-person workspaces and show it for shared ones.
#[must_use]
pub fn assignment_ui_mode(users: &Users) -> AssignmentUiMode {
    let visible = users.users.len() > 1;
    AssignmentUiMode {
        show_in_detail: visible,
        show_create_control: visible,
        show_reassign_control: visible,
        show_filter: visible,
    }
}
