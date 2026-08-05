//! Machine-local MCP connection material for logical workspace allowlists.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::workspace::WorkspaceId;

use super::CapabilityError;

mod runtime;
mod validation;

pub(crate) use runtime::{
    cleanup_claude_runtime_artifacts, cleanup_codex_runtime_artifacts,
    cleanup_workspace_capabilities, codex_mcp_launch, prepare_workspace_capabilities,
    write_claude_runtime_config,
};

/// Machine-local material available to one selected workspace.
#[derive(Clone, PartialEq, Eq)]
pub struct MachineCapabilityEnvironment {
    source_workspace: WorkspaceId,
    pub(crate) mcps: Vec<MachineMcp>,
    pub(crate) skills: Vec<MachineSkill>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
struct StoredCapabilities {
    mcps: Vec<MachineMcp>,
    skills: Vec<MachineSkill>,
}

/// One machine-local MCP connection.
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MachineMcp {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) command: Option<String>,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) credentials: McpCredentials,
}

/// Secret machine-local values needed by an MCP transport.
#[derive(Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct McpCredentials {
    pub(crate) environment: BTreeMap<String, Option<String>>,
    pub(crate) headers: BTreeMap<String, Option<String>>,
    pub(crate) bearer_token: Option<String>,
}

/// One machine-local source for a non-bundled skill.
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MachineSkill {
    pub(crate) name: String,
    pub(crate) path: std::path::PathBuf,
}

impl MachineCapabilityEnvironment {
    /// Parse selected-record capability material without copying it elsewhere.
    ///
    /// # Errors
    ///
    /// Returns [`CapabilityError::InvalidMachineEnvironment`] when the value
    /// does not match the machine capability schema.
    pub fn from_value(
        source_workspace: WorkspaceId,
        value: Value,
    ) -> Result<Self, CapabilityError> {
        let stored = serde_json::from_value::<StoredCapabilities>(value)
            .map_err(|error| CapabilityError::InvalidMachineEnvironment(error.to_string()))?;
        Ok(Self {
            source_workspace,
            mcps: stored.mcps,
            skills: stored.skills,
        })
    }

    #[must_use]
    pub(crate) const fn source_workspace(&self) -> WorkspaceId {
        self.source_workspace
    }

    pub(crate) fn from_selected_map(
        source_workspace: WorkspaceId,
        env: &serde_json::Map<String, Value>,
    ) -> Result<Self, CapabilityError> {
        Self::from_value(
            source_workspace,
            env.get("agent_capabilities")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({})),
        )
    }
}

impl std::fmt::Debug for MachineCapabilityEnvironment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MachineCapabilityEnvironment")
            .field("source_workspace", &self.source_workspace)
            .field(
                "mcps",
                &self.mcps.iter().map(|mcp| &mcp.name).collect::<Vec<_>>(),
            )
            .field(
                "skills",
                &self
                    .skills
                    .iter()
                    .map(|skill| &skill.name)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl std::fmt::Debug for MachineMcp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MachineMcp")
            .field("name", &self.name)
            .field(
                "transport",
                &if self.command.is_some() {
                    "stdio"
                } else {
                    "http"
                },
            )
            .field("connection", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for McpCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpCredentials")
            .field("environment_entries", &self.environment.len())
            .field("header_entries", &self.headers.len())
            .field(
                "bearer_token",
                &self.bearer_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl std::fmt::Debug for MachineSkill {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MachineSkill")
            .field("name", &self.name)
            .field("path", &"<redacted>")
            .finish()
    }
}

impl MachineMcp {
    pub(crate) fn unavailable_reason(&self) -> Option<String> {
        let command = self
            .command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let url = self
            .url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if command.is_some() == url.is_some() {
            return Some("machine connection must define exactly one command or URL".to_owned());
        }
        if command.is_some_and(|command| {
            command != self.command.as_deref().unwrap_or_default()
                || command.chars().any(char::is_whitespace)
                || command.chars().any(char::is_control)
        }) || self
            .args
            .iter()
            .any(|argument| argument.chars().any(char::is_control))
        {
            return Some(
                "stdio commands must be exact non-whitespace executables and arguments cannot contain controls"
                    .to_owned(),
            );
        }
        if url.is_some_and(|url| {
            url != self.url.as_deref().unwrap_or_default() || !validation::is_valid_http_url(url)
        }) {
            return Some("HTTP MCP URLs must be exact http(s) URLs with a valid host".to_owned());
        }
        if command.is_some()
            && (!self.credentials.headers.is_empty() || self.credentials.bearer_token.is_some())
            || url.is_some() && !self.credentials.environment.is_empty()
        {
            return Some("machine credentials do not match the selected MCP transport".to_owned());
        }
        if self
            .credentials
            .environment
            .iter()
            .chain(&self.credentials.headers)
            .any(|(name, value)| {
                name.trim().is_empty() || value.as_deref().map(str::trim).is_none_or(str::is_empty)
            })
            || self
                .credentials
                .bearer_token
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
        {
            return Some("one or more required machine credentials are unavailable".to_owned());
        }
        if self.credentials.environment.keys().any(|name| {
            !validation::is_environment_name(name)
                || validation::is_protected_frontend_environment(name)
        }) {
            return Some(
                "machine credential environment names must be valid and not reserved by the frontend"
                    .to_owned(),
            );
        }
        if self.credentials.headers.iter().any(|(name, value)| {
            !validation::is_http_header_name(name)
                || value
                    .as_deref()
                    .is_some_and(|value| value.chars().any(char::is_control))
        }) || self
            .credentials
            .bearer_token
            .as_deref()
            .is_some_and(|value| value.chars().any(char::is_control))
        {
            return Some("machine HTTP credentials contain invalid header data".to_owned());
        }
        None
    }

    pub(super) fn claude_value(&self) -> Value {
        let mut value = serde_json::Map::new();
        if let Some(command) = self.command.as_ref() {
            value.insert("type".to_owned(), Value::from("stdio"));
            value.insert("command".to_owned(), Value::from(command.clone()));
            if !self.args.is_empty() {
                value.insert(
                    "args".to_owned(),
                    Value::Array(self.args.iter().cloned().map(Value::from).collect()),
                );
            }
            let environment = present_values(&self.credentials.environment);
            if !environment.is_empty() {
                value.insert("env".to_owned(), Value::Object(environment));
            }
        } else if let Some(url) = self.url.as_ref() {
            value.insert("type".to_owned(), Value::from("http"));
            value.insert("url".to_owned(), Value::from(url.clone()));
            let mut headers = present_values(&self.credentials.headers);
            if let Some(token) = self.credentials.bearer_token.as_ref() {
                headers.insert(
                    "Authorization".to_owned(),
                    Value::from(format!("Bearer {token}")),
                );
            }
            if !headers.is_empty() {
                value.insert("headers".to_owned(), Value::Object(headers));
            }
        }
        Value::Object(value)
    }
}

fn present_values(values: &BTreeMap<String, Option<String>>) -> serde_json::Map<String, Value> {
    values
        .iter()
        .filter_map(|(name, value)| {
            value
                .as_ref()
                .map(|value| (name.clone(), Value::from(value.clone())))
        })
        .collect()
}
