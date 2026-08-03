//! Pure portable-user mutations shared by CLI and readiness setup.

use super::{
    EmailIdentity, PhoneIdentity, User, UserId, Users, UsersError, normalize_email, normalize_phone,
};

/// An inactive, reviewable proposal for converting legacy identity settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyUserMigration {
    pub user: User,
    pub unresolved_phones: Vec<String>,
    pub unresolved_emails: Vec<String>,
}

/// One validated in-memory portable user mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserMutation {
    Add(User),
    Update {
        id: UserId,
        name: Option<String>,
        add_phones: Vec<String>,
        add_emails: Vec<String>,
        response_email: Option<String>,
    },
    Remove(UserId),
}

/// Apply one mutation transactionally in memory.
pub fn apply_mutation(users: &mut Users, mutation: UserMutation) -> Result<(), UsersError> {
    let mut candidate = users.clone();
    match mutation {
        UserMutation::Add(user) => {
            let user = user.normalized()?;
            if candidate.user(&user.id).is_some() {
                return Err(UsersError::UserAlreadyExists {
                    user_id: user.id.to_string(),
                });
            }
            candidate.users.push(user);
        }
        UserMutation::Update {
            id,
            name,
            add_phones,
            add_emails,
            response_email,
        } => {
            let user = candidate
                .user_mut(&id)
                .ok_or_else(|| UsersError::UnknownUser {
                    user_id: id.to_string(),
                })?;
            if let Some(name) = name {
                user.name = name;
            }
            for value in add_phones {
                let value = normalize_phone(&value).map_err(|_| UsersError::InvalidPhone {
                    user_id: id.to_string(),
                    value,
                })?;
                user.phones.push(PhoneIdentity {
                    value,
                    inbound_allowed: true,
                });
            }
            for value in add_emails {
                let value = normalize_email(&value).map_err(|_| UsersError::InvalidEmail {
                    user_id: id.to_string(),
                    value,
                })?;
                user.emails.push(EmailIdentity {
                    value,
                    inbound_allowed: true,
                });
            }
            if let Some(value) = response_email {
                let value = normalize_email(&value).map_err(|_| UsersError::InvalidEmail {
                    user_id: id.to_string(),
                    value,
                })?;
                if !user.emails.iter().any(|email| email.value == value) {
                    user.emails.push(EmailIdentity {
                        value: value.clone(),
                        inbound_allowed: false,
                    });
                }
                user.response_email = Some(value);
            }
        }
        UserMutation::Remove(id) => {
            let Some(index) = candidate.users.iter().position(|user| user.id == id) else {
                return Err(UsersError::UnknownUser {
                    user_id: id.to_string(),
                });
            };
            if candidate.users.len() == 1 {
                return Err(UsersError::CannotRemoveLastUser {
                    user_id: id.to_string(),
                });
            }
            candidate.users.remove(index);
        }
    }
    super::validate::validate_users(&candidate)?;
    *users = candidate;
    Ok(())
}

/// Propose a lower-case kebab user ID from a display name.
#[must_use]
pub fn proposed_user_id(name: &str) -> String {
    let mut proposed = String::new();
    let mut separator_pending = false;
    for character in name.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            if separator_pending && !proposed.is_empty() {
                proposed.push('-');
            }
            proposed.push(character);
            separator_pending = false;
        } else if !proposed.is_empty() {
            separator_pending = true;
        }
    }
    proposed
}

/// Build, but do not persist, the first-user conversion from legacy settings.
///
/// The prior response address belongs to the named user. An allowlisted email
/// is enabled only when it exactly normalizes to that response address. Every
/// other legacy allowlist entry remains unresolved for an interactive mapping;
/// this helper never invents another person's name.
pub fn propose_legacy_user_migration(
    personalization_name: &str,
    id_override: Option<&str>,
    response_email: &str,
    allowed_phones: &[String],
    allowed_emails: &[String],
) -> Result<LegacyUserMigration, UsersError> {
    let name = personalization_name.trim();
    let proposed = proposed_user_id(name);
    let id_value = id_override.unwrap_or(&proposed);
    let id = UserId::parse(id_value).map_err(|_| UsersError::InvalidUserId {
        value: id_value.to_owned(),
    })?;
    if name.is_empty() {
        return Err(UsersError::EmptyName {
            user_id: id.to_string(),
        });
    }

    let mut unresolved_phones = Vec::new();
    for value in allowed_phones {
        let normalized = normalize_phone(value).map_err(|_| UsersError::InvalidPhone {
            user_id: id.to_string(),
            value: value.clone(),
        })?;
        if !unresolved_phones.contains(&normalized) {
            unresolved_phones.push(normalized);
        }
    }
    let legacy_response_email = if response_email.trim().is_empty() {
        None
    } else {
        Some(
            normalize_email(response_email).map_err(|_| UsersError::InvalidEmail {
                user_id: id.to_string(),
                value: response_email.to_owned(),
            })?,
        )
    };
    let mut unresolved_emails = Vec::new();
    for value in allowed_emails {
        let normalized = normalize_email(value).map_err(|_| UsersError::InvalidEmail {
            user_id: id.to_string(),
            value: value.clone(),
        })?;
        if legacy_response_email.as_deref() != Some(normalized.as_str())
            && !unresolved_emails.contains(&normalized)
        {
            unresolved_emails.push(normalized);
        }
    }
    let response_matches = legacy_response_email.as_ref().is_some_and(|response| {
        allowed_emails
            .iter()
            .any(|allowed| normalize_email(allowed).is_ok_and(|normalized| normalized == *response))
    });
    if let Some(unmatched) = legacy_response_email.as_ref().filter(|_| !response_matches)
        && !unresolved_emails.contains(unmatched)
    {
        unresolved_emails.push(unmatched.clone());
    }
    let response_email = legacy_response_email.filter(|_| response_matches);
    let emails = response_email
        .as_ref()
        .map(|value| {
            vec![EmailIdentity {
                value: value.clone(),
                inbound_allowed: true,
            }]
        })
        .unwrap_or_default();
    let user = User {
        id,
        name: name.to_owned(),
        phones: Vec::new(),
        emails,
        response_email,
    };
    let mut users = Users::empty();
    apply_mutation(&mut users, UserMutation::Add(user.clone()))?;
    Ok(LegacyUserMigration {
        user,
        unresolved_phones,
        unresolved_emails,
    })
}
