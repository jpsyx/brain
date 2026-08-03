//! Whole-registry validation and typed user errors.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use super::{USERS_SCHEMA_VERSION, Users};

/// A portable users schema, mutation, or storage operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsersError {
    UnsupportedSchema {
        found: u32,
    },
    InvalidJson {
        message: String,
    },
    InvalidUserId {
        value: String,
    },
    EmptyName {
        user_id: String,
    },
    DuplicateUserId {
        user_id: String,
    },
    InvalidPhone {
        user_id: String,
        value: String,
    },
    InvalidEmail {
        user_id: String,
        value: String,
    },
    DuplicatePhone {
        user_id: String,
        value: String,
    },
    DuplicateEmail {
        user_id: String,
        value: String,
    },
    DuplicateInboundPhone {
        value: String,
        first_user: String,
        second_user: String,
    },
    DuplicateInboundEmail {
        value: String,
        first_user: String,
        second_user: String,
    },
    ResponseEmailNotOnUser {
        user_id: String,
        value: String,
    },
    UnknownUser {
        user_id: String,
    },
    UserAlreadyExists {
        user_id: String,
    },
    CannotRemoveLastUser {
        user_id: String,
    },
    Io {
        operation: &'static str,
        path: PathBuf,
        related_path: Option<PathBuf>,
        kind: std::io::ErrorKind,
        message: String,
    },
}

impl UsersError {
    #[must_use]
    pub fn is_missing_store(&self) -> bool {
        matches!(
            self,
            Self::Io {
                operation: "read portable users",
                kind: std::io::ErrorKind::NotFound,
                ..
            }
        )
    }
}

impl Display for UsersError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchema { found } => write!(
                formatter,
                "unsupported portable users schema {found}; expected {USERS_SCHEMA_VERSION}"
            ),
            Self::InvalidJson { message } => write!(formatter, "invalid users JSON: {message}"),
            Self::InvalidUserId { value } => write!(
                formatter,
                "invalid user ID `{value}`; use lower-case kebab case such as `alex-smith`"
            ),
            Self::EmptyName { user_id } => write!(formatter, "user {user_id} needs a display name"),
            Self::DuplicateUserId { user_id } => {
                write!(formatter, "portable user ID {user_id} is not unique")
            }
            Self::InvalidPhone { user_id, value } => write!(
                formatter,
                "user {user_id} phone `{value}` is not unambiguous E.164 or North American format"
            ),
            Self::InvalidEmail { user_id, value } => {
                write!(formatter, "user {user_id} email `{value}` is invalid")
            }
            Self::DuplicatePhone { user_id, value } => {
                write!(formatter, "user {user_id} repeats phone {value}")
            }
            Self::DuplicateEmail { user_id, value } => {
                write!(formatter, "user {user_id} repeats email {value}")
            }
            Self::DuplicateInboundPhone {
                value,
                first_user,
                second_user,
            } => write!(
                formatter,
                "enabled phone {value} identifies both {first_user} and {second_user}"
            ),
            Self::DuplicateInboundEmail {
                value,
                first_user,
                second_user,
            } => write!(
                formatter,
                "enabled email {value} identifies both {first_user} and {second_user}"
            ),
            Self::ResponseEmailNotOnUser { user_id, value } => write!(
                formatter,
                "response email {value} is not one of user {user_id}'s normalized emails"
            ),
            Self::UnknownUser { user_id } => write!(formatter, "unknown portable user {user_id}"),
            Self::UserAlreadyExists { user_id } => {
                write!(formatter, "portable user {user_id} already exists")
            }
            Self::CannotRemoveLastUser { user_id } => {
                write!(formatter, "cannot remove the last portable user {user_id}")
            }
            Self::Io {
                operation,
                path,
                related_path,
                message,
                ..
            } => {
                write!(formatter, "failed to {operation} at {}", path.display())?;
                if let Some(related_path) = related_path {
                    write!(formatter, " using {}", related_path.display())?;
                }
                write!(formatter, ": {message}")
            }
        }
    }
}

impl Error for UsersError {}

pub(crate) fn validate_users(users: &Users) -> Result<(), UsersError> {
    if users.schema_version != USERS_SCHEMA_VERSION {
        return Err(UsersError::UnsupportedSchema {
            found: users.schema_version,
        });
    }
    let mut ids = BTreeSet::new();
    let mut inbound_phones = BTreeMap::<&str, &str>::new();
    let mut inbound_emails = BTreeMap::<&str, &str>::new();
    for user in &users.users {
        let id = user.id.as_str();
        if !ids.insert(id) {
            return Err(UsersError::DuplicateUserId {
                user_id: id.to_owned(),
            });
        }
        if user.name.trim().is_empty() {
            return Err(UsersError::EmptyName {
                user_id: id.to_owned(),
            });
        }
        let mut own_phones = BTreeSet::new();
        for phone in &user.phones {
            if !own_phones.insert(phone.value.as_str()) {
                return Err(UsersError::DuplicatePhone {
                    user_id: id.to_owned(),
                    value: phone.value.clone(),
                });
            }
            if phone.inbound_allowed
                && let Some(first_user) = inbound_phones.insert(&phone.value, id)
            {
                return Err(UsersError::DuplicateInboundPhone {
                    value: phone.value.clone(),
                    first_user: first_user.to_owned(),
                    second_user: id.to_owned(),
                });
            }
        }
        let mut own_emails = BTreeSet::new();
        for email in &user.emails {
            if !own_emails.insert(email.value.as_str()) {
                return Err(UsersError::DuplicateEmail {
                    user_id: id.to_owned(),
                    value: email.value.clone(),
                });
            }
            if email.inbound_allowed
                && let Some(first_user) = inbound_emails.insert(&email.value, id)
            {
                return Err(UsersError::DuplicateInboundEmail {
                    value: email.value.clone(),
                    first_user: first_user.to_owned(),
                    second_user: id.to_owned(),
                });
            }
        }
        if let Some(response_email) = user.response_email.as_deref()
            && !own_emails.contains(response_email)
        {
            return Err(UsersError::ResponseEmailNotOnUser {
                user_id: id.to_owned(),
                value: response_email.to_owned(),
            });
        }
    }
    Ok(())
}
