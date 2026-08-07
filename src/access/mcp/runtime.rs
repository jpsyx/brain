use std::fmt::Write as FmtWrite;
use std::fs::{self, OpenOptions};
use std::io::Write as IoWrite;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use crate::access::{CapabilityError, CapabilityPlan};
use crate::workspace::{WorkspaceContext, WorkspaceId};

pub(crate) fn prepare_workspace_capabilities(
    workspace: &WorkspaceContext,
) -> Result<(), CapabilityError> {
    crate::access::ensure_capability_directory(workspace, &workspace.paths().capabilities_dir())
}

pub(crate) fn cleanup_workspace_capabilities(
    workspace: &WorkspaceContext,
) -> Result<(), CapabilityError> {
    let path = workspace.paths().capabilities_dir();
    if crate::access::remove_capability_path(workspace, &path)? {
        sync_parent(&path)?;
    }
    Ok(())
}

pub(crate) fn cleanup_claude_runtime_artifacts(
    workspace: &WorkspaceContext,
) -> Result<(), CapabilityError> {
    let directory = workspace.paths().capabilities_dir();
    cleanup_claude_artifacts_in(
        workspace,
        &directory,
        Some(&workspace.paths().capability_mcp_config()),
    )
}

pub(crate) fn cleanup_codex_runtime_artifacts(
    workspace: &WorkspaceContext,
) -> Result<(), CapabilityError> {
    let path = workspace.paths().capabilities_dir().join("codex-mcp");
    if crate::access::remove_capability_path(workspace, &path)? {
        sync_parent(&path)?;
    }
    Ok(())
}

pub(crate) fn write_claude_runtime_config(
    workspace: &WorkspaceContext,
    plan: &CapabilityPlan,
) -> Result<(), CapabilityError> {
    let path = workspace.paths().capability_mcp_config();
    let parent = path.parent().ok_or_else(|| {
        CapabilityError::RuntimeArtifact("runtime MCP path has no parent".to_owned())
    })?;
    crate::access::ensure_capability_directory(workspace, parent)?;
    cleanup_claude_artifacts_in(workspace, parent, None)?;
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
        fs::rename(&temporary, &path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
        let _ = sync_directory(parent);
    }
    result.map_err(|error| CapabilityError::RuntimeArtifact(error.to_string()))
}

pub(crate) struct CodexMcpLaunch {
    pub(crate) overrides: Vec<String>,
    pub(crate) environment: Vec<(String, String)>,
}

pub(crate) struct OpenCodeMcpLaunch {
    pub(crate) entries: serde_json::Map<String, serde_json::Value>,
    pub(crate) environment: Vec<(String, String)>,
}

pub(crate) fn opencode_mcp_launch(plan: &CapabilityPlan) -> OpenCodeMcpLaunch {
    let mut entries = serde_json::Map::new();
    let mut environment = Vec::new();
    for connection in plan.mcps.available_connections() {
        let server_name =
            isolated_server_name(plan.credentials.source_workspace(), &connection.name);
        let mut value = serde_json::Map::new();
        if let Some(command) = connection.command.as_ref() {
            value.insert("type".to_owned(), serde_json::Value::from("local"));
            value.insert(
                "command".to_owned(),
                serde_json::Value::Array(
                    std::iter::once(command)
                        .chain(&connection.args)
                        .cloned()
                        .map(serde_json::Value::from)
                        .collect(),
                ),
            );
            let variables = connection
                .credentials
                .environment
                .iter()
                .enumerate()
                .filter_map(|(index, (target, value))| {
                    value.as_ref().map(|secret| {
                        let source = generated_secret_name(&connection.name, "ENV", index);
                        environment.push((source.clone(), secret.clone()));
                        (
                            target.clone(),
                            serde_json::Value::from(format!("{{env:{source}}}")),
                        )
                    })
                })
                .collect::<serde_json::Map<_, _>>();
            if !variables.is_empty() {
                value.insert(
                    "environment".to_owned(),
                    serde_json::Value::Object(variables),
                );
            }
        } else if let Some(url) = connection.url.as_ref() {
            value.insert("type".to_owned(), serde_json::Value::from("remote"));
            value.insert("url".to_owned(), serde_json::Value::from(url.clone()));
            let mut headers = connection
                .credentials
                .headers
                .iter()
                .enumerate()
                .filter_map(|(index, (header, value))| {
                    value.as_ref().map(|secret| {
                        let source = generated_secret_name(&connection.name, "HEADER", index);
                        environment.push((source.clone(), secret.clone()));
                        (
                            header.clone(),
                            serde_json::Value::from(format!("{{env:{source}}}")),
                        )
                    })
                })
                .collect::<serde_json::Map<_, _>>();
            if let Some(secret) = connection.credentials.bearer_token.as_ref() {
                let source = generated_secret_name(&connection.name, "BEARER", 0);
                environment.push((source.clone(), secret.clone()));
                headers.insert(
                    "Authorization".to_owned(),
                    serde_json::Value::from(format!("Bearer {{env:{source}}}")),
                );
            }
            if !headers.is_empty() {
                value.insert("headers".to_owned(), serde_json::Value::Object(headers));
            }
        }
        value.insert("enabled".to_owned(), serde_json::Value::Bool(true));
        entries.insert(server_name, serde_json::Value::Object(value));
    }
    OpenCodeMcpLaunch {
        entries,
        environment,
    }
}

pub(crate) fn codex_mcp_launch(
    workspace: &WorkspaceContext,
    plan: &CapabilityPlan,
) -> Result<CodexMcpLaunch, CapabilityError> {
    let mut overrides = Vec::new();
    let mut environment = Vec::new();
    prepare_workspace_capabilities(workspace)?;
    let wrappers_dir = workspace.paths().capabilities_dir().join("codex-mcp");
    cleanup_codex_runtime_artifacts(workspace)?;
    crate::access::ensure_capability_directory(workspace, &wrappers_dir)?;
    let result = (|| -> Result<(), CapabilityError> {
        for connection in plan.mcps.available_connections() {
            let server_name =
                isolated_server_name(plan.credentials.source_workspace(), &connection.name);
            let prefix = format!("mcp_servers.{server_name}");
            if let Some(command) = connection.command.as_ref() {
                let mut mappings = Vec::new();
                for (index, (name, value)) in connection.credentials.environment.iter().enumerate()
                {
                    if let Some(value) = value.as_ref() {
                        let variable = generated_secret_name(&connection.name, "ENV", index);
                        environment.push((variable.clone(), value.clone()));
                        mappings.push((name.clone(), variable));
                    }
                }
                let wrapper = wrappers_dir.join(format!("{server_name}.sh"));
                write_codex_stdio_wrapper(&wrapper, command, &connection.args, &mappings)?;
                overrides.push(format!(
                    "{prefix}.command={}",
                    serde_json::to_string(&wrapper.display().to_string())
                        .expect("string serialization")
                ));
                let names = mappings
                    .into_iter()
                    .map(|(_, variable)| variable)
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
        sync_directory(&wrappers_dir)?;
        sync_parent(&wrappers_dir)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = crate::access::remove_capability_path(workspace, &wrappers_dir);
        let _ = sync_parent(&wrappers_dir);
        return Err(error);
    }
    Ok(CodexMcpLaunch {
        overrides,
        environment,
    })
}

fn cleanup_claude_artifacts_in(
    workspace: &WorkspaceContext,
    directory: &Path,
    live_config: Option<&Path>,
) -> Result<(), CapabilityError> {
    if !crate::access::existing_capability_directory(workspace, directory)? {
        return Ok(());
    }
    let mut removed = false;
    if let Some(path) = live_config {
        removed |= crate::access::remove_capability_path(workspace, path)?;
    }
    for entry in fs::read_dir(directory)
        .map_err(|error| CapabilityError::RuntimeArtifact(error.to_string()))?
    {
        let entry = entry.map_err(|error| CapabilityError::RuntimeArtifact(error.to_string()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".claude-mcp-") && name.ends_with(".tmp") {
            removed |= crate::access::remove_capability_path(workspace, &entry.path())?;
        }
    }
    if removed {
        sync_directory(directory)?;
    }
    Ok(())
}

fn sync_parent(path: &Path) -> Result<(), CapabilityError> {
    let parent = path.parent().ok_or_else(|| {
        CapabilityError::RuntimeArtifact(format!("{} has no parent", path.display()))
    })?;
    sync_directory(parent)
}

fn sync_directory(path: &Path) -> Result<(), CapabilityError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| CapabilityError::RuntimeArtifact(error.to_string()))
}

fn write_codex_stdio_wrapper(
    path: &Path,
    command: &str,
    args: &[String],
    mappings: &[(String, String)],
) -> Result<(), CapabilityError> {
    let mut script = "#!/bin/sh\nset -eu\n".to_owned();
    for (target, source) in mappings {
        writeln!(&mut script, "export {target}=\"${{{source}}}\"")
            .expect("writing to a String cannot fail");
        writeln!(&mut script, "unset {source}").expect("writing to a String cannot fail");
    }
    write!(&mut script, "exec {}", shell_quote(command)).expect("writing to a String cannot fail");
    for argument in args {
        write!(&mut script, " {}", shell_quote(argument)).expect("writing to a String cannot fail");
    }
    script.push('\n');
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o700)
        .open(path)
        .map_err(|error| CapabilityError::RuntimeArtifact(error.to_string()))?;
    file.write_all(script.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| CapabilityError::RuntimeArtifact(error.to_string()))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn isolated_server_name(workspace: WorkspaceId, logical_name: &str) -> String {
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
