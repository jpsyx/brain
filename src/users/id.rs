//! Validated portable user IDs.

use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize};

/// A workspace-local, portable person identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct UserId(String);

impl UserId {
    /// Parse an exact lower-case kebab identifier.
    pub fn parse(value: &str) -> Result<Self, UserIdError> {
        if value.is_empty()
            || value.starts_with('-')
            || value.ends_with('-')
            || value.split('-').any(str::is_empty)
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(UserIdError {
                value: value.to_owned(),
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// Borrow the canonical ID.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for UserId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for UserId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// A user ID is not exact lower-case kebab case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserIdError {
    value: String,
}

impl Display for UserIdError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid user ID `{}`; use lower-case kebab case such as `alex-smith`",
            self.value
        )
    }
}

impl Error for UserIdError {}
