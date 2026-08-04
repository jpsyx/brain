//! Machine-local MCP connection material for logical workspace allowlists.

use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::fs::{self, OpenOptions};
use std::io::Write as IoWrite;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::workspace::WorkspaceId;

use super::CapabilityError;

/// Machine-local material available to one selected workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct McpCredentials {
    pub(crate) environment: BTreeMap<String, Option<String>>,
    pub(crate) headers: BTreeMap<String, Option<String>>,
    pub(crate) bearer_token: Option<String>,
}

/// One machine-local source for a non-bundled skill.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
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

impl MachineMcp {
    pub(crate) fn unavailable_reason(&self) -> Option<String> {
        let command = self
            .command
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let url = self.url.as_deref().map(str::trim).filter(|v| !v.is_empty());
        if command.is_some() == url.is_some() {
            return Some("machine connection must define exactly one command or URL".to_owned());
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
        None
    }

    fn claude_value(&self) -> Value {
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

pub(crate) fn write_claude_runtime_config(
    path: &Path,
    plan: &super::CapabilityPlan,
) -> Result<(), CapabilityError> {
    let parent = path.parent().ok_or_else(|| {
        CapabilityError::RuntimeArtifact("runtime MCP path has no parent".to_owned())
    })?;
    fs::create_dir_all(parent)
        .and_then(|()| fs::set_permissions(parent, fs::Permissions::from_mode(0o700)))
        .map_err(|error| CapabilityError::RuntimeArtifact(error.to_string()))?;
    let servers = plan
        .mcps
        .available_connections()
        .map(|connection| (connection.name.clone(), connection.claude_value()))
        .collect::<serde_json::Map<_, _>>();
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({"mcpServers": servers}))
        .map_err(|error| CapabilityError::RuntimeArtifact(error.to_string()))?;
    let temporary = parent.join(format!(".claude-mcp-{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| CapabilityError::RuntimeArtifact(error.to_string()))
}

pub(crate) struct CodexMcpLaunch {
    pub(crate) overrides: Vec<String>,
    pub(crate) environment: Vec<(String, String)>,
}

pub(crate) fn codex_mcp_launch(plan: &super::CapabilityPlan) -> CodexMcpLaunch {
    let mut overrides = Vec::new();
    let mut environment = Vec::new();
    for connection in plan.mcps.available_connections() {
        let prefix = format!(
            "mcp_servers.{}",
            codex_server_name(plan.credentials.source_workspace(), &connection.name)
        );
        if let Some(command) = connection.command.as_ref() {
            overrides.push(format!(
                "{prefix}.command={}",
                serde_json::to_string(command).expect("string serialization")
            ));
            if !connection.args.is_empty() {
                overrides.push(format!(
                    "{prefix}.args={}",
                    serde_json::to_string(&connection.args).expect("argument serialization")
                ));
            }
            let names = connection
                .credentials
                .environment
                .iter()
                .filter_map(|(name, value)| {
                    value.as_ref().map(|value| {
                        environment.push((name.clone(), value.clone()));
                        name.clone()
                    })
                })
                .collect::<Vec<_>>();
            if !names.is_empty() {
                overrides.push(format!(
                    "{prefix}.env_vars={}",
                    serde_json::to_string(&names).expect("environment name serialization")
                ));
            }
        } else if let Some(url) = connection.url.as_ref() {
            overrides.push(format!(
                "{prefix}.url={}",
                serde_json::to_string(url).expect("URL serialization")
            ));
            let headers = connection
                .credentials
                .headers
                .iter()
                .enumerate()
                .filter_map(|(index, (header, value))| {
                    value.as_ref().map(|value| {
                        let variable = generated_secret_name(&connection.name, "HEADER", index);
                        environment.push((variable.clone(), value.clone()));
                        (header.clone(), variable)
                    })
                })
                .collect::<Vec<_>>();
            if !headers.is_empty() {
                overrides.push(format!(
                    "{prefix}.env_http_headers={}",
                    toml_inline_table(&headers)
                ));
            }
            if let Some(token) = connection.credentials.bearer_token.as_ref() {
                let variable = generated_secret_name(&connection.name, "BEARER", 0);
                environment.push((variable.clone(), token.clone()));
                overrides.push(format!(
                    "{prefix}.bearer_token_env_var={}",
                    serde_json::to_string(&variable).expect("environment name serialization")
                ));
            }
        }
        overrides.push(format!("{prefix}.enabled=true"));
    }
    CodexMcpLaunch {
        overrides,
        environment,
    }
}

fn codex_server_name(workspace: WorkspaceId, logical_name: &str) -> String {
    let workspace = workspace
        .to_string()
        .chars()
        .filter(char::is_ascii_hexdigit)
        .collect::<String>();
    let mut encoded = String::with_capacity(logical_name.len() * 2);
    for byte in logical_name.as_bytes() {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    format!("brain_ws_{workspace}_{encoded}")
}

fn generated_secret_name(server: &str, kind: &str, index: usize) -> String {
    let mut encoded = String::with_capacity(server.len() * 2);
    for byte in server.as_bytes() {
        write!(&mut encoded, "{byte:02X}").expect("writing to a String cannot fail");
    }
    format!("BRAIN_MCP_{encoded}_{kind}_{index}")
}

fn toml_inline_table(entries: &[(String, String)]) -> String {
    let body = entries
        .iter()
        .map(|(key, value)| {
            format!(
                "{} = {}",
                serde_json::to_string(key).expect("header serialization"),
                serde_json::to_string(value).expect("environment name serialization")
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {body} }}")
}
