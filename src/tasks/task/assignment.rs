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
