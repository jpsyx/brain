//! Portable access-mode values.

use serde::Deserialize;

/// A workspace's portable access policy.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    /// Preserve the normal trusted personal Brain behavior.
    #[default]
    Unrestricted,
    /// Apply advisory root guidance and best-effort capability filtering.
    WorkspaceOnly,
}

impl AccessMode {
    /// Parse the stable portable-config representation.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "unrestricted" => Some(Self::Unrestricted),
            "workspace_only" => Some(Self::WorkspaceOnly),
            _ => None,
        }
    }

    /// Stable value persisted in portable config.
    #[must_use]
    pub const fn as_config_value(self) -> &'static str {
        match self {
            Self::Unrestricted => "unrestricted",
            Self::WorkspaceOnly => "workspace_only",
        }
    }
}
