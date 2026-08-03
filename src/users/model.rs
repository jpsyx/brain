//! Serializable portable user schema and trusted parse boundary.

use serde::{Deserialize, Serialize};

use super::{UserId, UsersError, normalize_email, normalize_phone};

/// The only portable users schema accepted by this release.
pub const USERS_SCHEMA_VERSION: u32 = 1;

/// Every portable member of one workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Users {
    pub schema_version: u32,
    pub users: Vec<User>,
}

/// One portable person. IDs identify people, not devices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub phones: Vec<PhoneIdentity>,
    pub emails: Vec<EmailIdentity>,
    pub response_email: Option<String>,
}

/// One normalized phone identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhoneIdentity {
    pub value: String,
    #[serde(default)]
    pub inbound_allowed: bool,
}

/// One normalized email identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailIdentity {
    pub value: String,
    #[serde(default)]
    pub inbound_allowed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUsers {
    schema_version: u32,
    users: Vec<RawUser>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUser {
    id: String,
    name: String,
    #[serde(default)]
    phones: Vec<PhoneIdentity>,
    #[serde(default)]
    emails: Vec<EmailIdentity>,
    #[serde(default)]
    response_email: Option<String>,
}

impl Users {
    /// Construct an empty current-schema registry.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            schema_version: USERS_SCHEMA_VERSION,
            users: Vec::new(),
        }
    }

    /// Parse and validate a complete portable registry.
    pub fn parse(bytes: &[u8]) -> Result<Self, UsersError> {
        let raw: RawUsers =
            serde_json::from_slice(bytes).map_err(|error| UsersError::InvalidJson {
                message: error.to_string(),
            })?;
        let users = raw
            .users
            .into_iter()
            .map(User::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let users = Self {
            schema_version: raw.schema_version,
            users,
        };
        super::validate::validate_users(&users)?;
        Ok(users)
    }

    /// Serialize canonical pretty JSON with one trailing newline.
    pub fn to_bytes(&self) -> Result<Vec<u8>, UsersError> {
        super::validate::validate_users(self)?;
        let mut bytes =
            serde_json::to_vec_pretty(self).map_err(|error| UsersError::InvalidJson {
                message: error.to_string(),
            })?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Find one user by validated ID.
    #[must_use]
    pub fn user(&self, id: &UserId) -> Option<&User> {
        self.users.iter().find(|user| user.id == *id)
    }

    /// Find one user mutably by validated ID.
    #[must_use]
    pub fn user_mut(&mut self, id: &UserId) -> Option<&mut User> {
        self.users.iter_mut().find(|user| user.id == *id)
    }

    /// Resolve an enabled inbound phone identity.
    #[must_use]
    pub fn resolve_phone(&self, value: &str) -> Option<&User> {
        let normalized = normalize_phone(value).ok()?;
        self.users.iter().find(|user| {
            user.phones
                .iter()
                .any(|phone| phone.inbound_allowed && phone.value == normalized)
        })
    }

    /// Resolve an enabled inbound email identity.
    #[must_use]
    pub fn resolve_email(&self, value: &str) -> Option<&User> {
        let normalized = normalize_email(value).ok()?;
        self.users.iter().find(|user| {
            user.emails
                .iter()
                .any(|email| email.inbound_allowed && email.value == normalized)
        })
    }
}

impl TryFrom<RawUser> for User {
    type Error = UsersError;

    fn try_from(raw: RawUser) -> Result<Self, Self::Error> {
        let id = UserId::parse(&raw.id).map_err(|_| UsersError::InvalidUserId {
            value: raw.id.clone(),
        })?;
        let mut phones = raw.phones;
        for phone in &mut phones {
            phone.value = normalize_phone(&phone.value).map_err(|_| UsersError::InvalidPhone {
                user_id: id.to_string(),
                value: phone.value.clone(),
            })?;
        }
        let mut emails = raw.emails;
        for email in &mut emails {
            email.value = normalize_email(&email.value).map_err(|_| UsersError::InvalidEmail {
                user_id: id.to_string(),
                value: email.value.clone(),
            })?;
        }
        let response_email = raw
            .response_email
            .map(|value| {
                normalize_email(&value).map_err(|_| UsersError::InvalidEmail {
                    user_id: id.to_string(),
                    value,
                })
            })
            .transpose()?;
        Ok(Self {
            id,
            name: raw.name.trim().to_owned(),
            phones,
            emails,
            response_email,
        })
    }
}

impl User {
    pub(crate) fn normalized(mut self) -> Result<Self, UsersError> {
        let raw = RawUser {
            id: self.id.to_string(),
            name: std::mem::take(&mut self.name),
            phones: std::mem::take(&mut self.phones),
            emails: std::mem::take(&mut self.emails),
            response_email: self.response_email.take(),
        };
        Self::try_from(raw)
    }
}
