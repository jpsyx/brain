//! Preserve Codex's native layered developer instructions without loading integrations.

use crate::agent::{AgentError, command_probe::ShellProbeRunner};
use serde_json::Value;
use std::{path::Path, time::Duration};

const OUTPUT_LIMIT: usize = 1024 * 1024;
const DIAGNOSTIC: &str = "cannot safely read Codex developer instructions; use a compatible Codex version with `mcp list --json` and tagged `debug prompt-input`, and a codex_cmd executable that forwards arguments. Existing instructions were not replaced";

fn error() -> AgentError {
    AgentError::Frontend(DIAGNOSTIC.to_owned())
}

pub(super) fn compose(
    command: &str,
    cwd: &Path,
    environment: &[(String, String)],
    payload: &str,
) -> Result<String, AgentError> {
    if !crate::agent::direct_command::accepts_appended_options(command) {
        return Err(error());
    }
    let runner = ShellProbeRunner::with_limits(Duration::from_secs(5), OUTPUT_LIMIT);
    let run = |arguments: &[&str]| {
        let bytes = arguments.iter().fold(command.len(), |total, arg| {
            total.saturating_add(crate::agent::frontend::shell_quote(arg).len() + 1)
        });
        if bytes > crate::agent::frontend::SHELL_COMMAND_ARGUMENT_BUDGET_BYTES {
            return Err(error());
        }
        let output = runner
            .run_at_with_environment(command, arguments, Some(cwd), Some(environment))
            .map_err(|_| error())?;
        if !output.success || output.stdout.len() >= OUTPUT_LIMIT {
            return Err(error());
        }
        serde_json::from_str::<Value>(&output.stdout).map_err(|_| error())
    };
    let inventory = run(&["mcp", "list", "--json"])?;
    let mut arguments = vec!["debug".to_owned(), "prompt-input".to_owned()];
    let mut disabled_servers = Vec::new();
    for server in inventory.as_array().ok_or_else(error)? {
        let name = server
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(error)?;
        if name.is_empty() || !server.get("enabled").is_some_and(Value::is_boolean) {
            return Err(error());
        }
        let key = serde_json::to_string(name).map_err(|_| error())?;
        disabled_servers.push(format!("{key}={{enabled=false}}"));
    }
    arguments.extend([
        "-c".to_owned(),
        format!("mcp_servers={{{}}}", disabled_servers.join(",")),
    ]);
    for feature in ["apps", "plugins", "hooks", "shell_snapshot"] {
        arguments.extend(["--disable".to_owned(), feature.to_owned()]);
    }
    let output = run(&arguments.iter().map(String::as_str).collect::<Vec<_>>())?;
    let existing = developer_instructions(&output)?;
    let combined = existing.map_or_else(
        || payload.to_owned(),
        |prior| format!("{prior}\n\n{payload}"),
    );
    crate::agent::instructions::validate_size(&combined)?;
    Ok(combined)
}

fn developer_instructions(value: &Value) -> Result<Option<&str>, AgentError> {
    let mut found = None;
    let mut recognized = false;
    for message in value.as_array().ok_or_else(error)? {
        if message.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let content = message
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(error)?;
        let metadata = message
            .get("internal_chat_message_metadata_passthrough")
            .ok_or_else(error)?;
        let kinds = metadata
            .get("content_item_kinds")
            .and_then(Value::as_array)
            .ok_or_else(error)?;
        let origins = metadata
            .get("content_item_metadata")
            .and_then(Value::as_array)
            .ok_or_else(error)?;
        if content.len() != kinds.len() || kinds.len() != origins.len() {
            return Err(error());
        }
        for ((item, kind), origin) in content.iter().zip(kinds).zip(origins) {
            let kind = kind.as_str().ok_or_else(error)?;
            let provenance = &origin["provenance"];
            if kind == "generic.developer_instructions" {
                if message["role"] != "developer"
                    || provenance["type"] != "developer_instructions"
                    || !provenance["from_additional_requirements"].is_boolean()
                    || item["type"] != "input_text"
                {
                    return Err(error());
                }
                recognized = true;
                if provenance["from_additional_requirements"] == false {
                    if found.is_some() {
                        return Err(error());
                    }
                    found = Some(item["text"].as_str().ok_or_else(error)?);
                }
            } else if provenance["type"] == "developer_instructions" {
                return Err(error());
            } else if matches!(kind, "permissions.instructions" | "model.base_instructions")
                && provenance["type"] == "harness"
                && message["role"] == "developer"
            {
                recognized = true;
            }
        }
    }
    if !recognized {
        return Err(error());
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(kind: &str, provenance: &Value, text: &str) -> Value {
        serde_json::json!([{
            "type":"message", "role":"developer",
            "content":[{"type":"input_text","text":text}],
            "internal_chat_message_metadata_passthrough": {
                "content_item_kinds":[kind], "content_item_metadata":[{"provenance":provenance}]
            }
        }])
    }

    #[test]
    fn tagged_absence_keeps_frontend_owned_instructions_out_of_the_override() {
        let value = message(
            "permissions.instructions",
            &serde_json::json!({"type":"harness"}),
            "Built-in policy",
        );
        assert_eq!(developer_instructions(&value).unwrap(), None);
    }

    #[test]
    fn unsupported_or_malformed_provenance_never_means_no_instructions() {
        let valid = message(
            "generic.developer_instructions",
            &serde_json::json!({"type":"developer_instructions","from_additional_requirements":false}),
            "Keep these",
        );
        let mut mismatched = valid.clone();
        mismatched[0]["content"].as_array_mut().unwrap().clear();
        let mut missing = valid.clone();
        missing[0]
            .as_object_mut()
            .unwrap()
            .remove("internal_chat_message_metadata_passthrough");
        let mut wrong_role = message(
            "permissions.instructions",
            &serde_json::json!({"type":"harness"}),
            "Not a developer message",
        );
        wrong_role[0]["role"] = serde_json::json!("user");
        let wrong_origin = message(
            "generic.developer_instructions",
            &serde_json::json!({"type":"harness"}),
            "Keep these",
        );
        for value in [
            serde_json::json!([]),
            mismatched,
            missing,
            wrong_role,
            wrong_origin,
        ] {
            assert!(developer_instructions(&value).is_err(), "{value}");
        }
        assert_eq!(developer_instructions(&valid).unwrap(), Some("Keep these"));
    }
}
