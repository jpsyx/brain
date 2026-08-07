//! Merge Brain's reserved OpenCode launch layer into inherited inline config.

use std::path::Path;

use serde_json::{Map, Value};

use crate::agent::AgentError;

const INVALID_CONFIG: &str = "OPENCODE_CONFIG_CONTENT must be a valid JSON object";

pub(super) fn parse(inherited: Option<&str>) -> Result<Map<String, Value>, AgentError> {
    let Some(raw) = inherited.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(Map::new());
    };
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| AgentError::Frontend(INVALID_CONFIG.to_owned()))
}

pub(super) fn merge(
    mut inherited: Map<String, Value>,
    brain_prompt: &str,
    mcp_entries: Option<Map<String, Value>>,
    selected_skills: Option<(&Path, &[&str])>,
) -> Result<String, AgentError> {
    let mut brain = Map::from_iter([
        ("mode".to_owned(), Value::from("primary")),
        ("prompt".to_owned(), Value::from(brain_prompt)),
    ]);
    if let Some((skills_dir, names)) = selected_skills {
        let mut skill_permissions = Map::from_iter([("*".to_owned(), Value::from("deny"))]);
        skill_permissions.extend(
            names
                .iter()
                .map(|name| ((*name).to_owned(), Value::from("allow"))),
        );
        brain.insert(
            "permission".to_owned(),
            serde_json::json!({"skill": skill_permissions}),
        );
        merge_skill_path(&mut inherited, skills_dir)?;
    }
    object_entry(&mut inherited, "agent")?.insert("brain".to_owned(), Value::Object(brain));
    inherited.insert("default_agent".to_owned(), Value::from("brain"));

    if inherited.contains_key("mcp") {
        object_entry(&mut inherited, "mcp")?.retain(|name, _| !name.starts_with("brain_ws_"));
    }
    if let Some(entries) = mcp_entries {
        if !entries.is_empty() {
            object_entry(&mut inherited, "mcp")?.extend(entries);
        }
    }
    serde_json::to_string(&Value::Object(inherited))
        .map_err(|error| AgentError::Frontend(format!("serialize OpenCode config: {error}")))
}

fn merge_skill_path(config: &mut Map<String, Value>, skills_dir: &Path) -> Result<(), AgentError> {
    let skills = object_entry(config, "skills")?;
    let paths = skills
        .entry("paths".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| AgentError::Frontend(INVALID_CONFIG.to_owned()))?;
    let path = Value::from(skills_dir.display().to_string());
    if !paths.contains(&path) {
        paths.push(path);
    }
    Ok(())
}

fn object_entry<'a>(
    config: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, AgentError> {
    config
        .entry(key.to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| AgentError::Frontend(INVALID_CONFIG.to_owned()))
}

pub(super) fn compatibility_probe(directory: &Path) -> Result<String, AgentError> {
    let workspace_id = crate::workspace::WorkspaceId::parse("00000000-0000-4000-8000-000000000001")
        .expect("static compatibility workspace ID");
    let machine = crate::access::MachineCapabilityEnvironment::from_value(
        workspace_id,
        serde_json::json!({
            "mcps": [{
                "name": "probe",
                "command": "/usr/bin/true",
                "args": ["--version"]
            }]
        }),
    )
    .map_err(|error| AgentError::Frontend(error.to_string()))?;
    let plan = crate::access::capability_plan(
        &crate::config::Config {
            access_mode: crate::access::AccessMode::WorkspaceOnly,
            allowed_mcps: vec!["probe".to_owned()],
            allowed_skills: vec!["todo".to_owned()],
            ..crate::config::Config::default()
        },
        &machine,
    )
    .map_err(|error| AgentError::Frontend(error.to_string()))?;
    let mcp = crate::access::opencode_mcp_launch(&plan);
    let skills = plan.skills.available_names();
    merge(
        Map::new(),
        "Brain compatibility probe",
        Some(mcp.entries),
        Some((directory, &skills)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preserves_user_config_and_replaces_only_brain_reserved_entries() {
        let inherited = serde_json::json!({
            "theme": "catppuccin",
            "agent": {
                "review": {"mode": "subagent", "description": "Keep me"},
                "brain": {"model": "replace me"}
            },
            "mcp": {
                "brain_ws_stale": {"type": "local", "command": ["stale"]},
                "user-server": {"type": "remote", "url": "https://user.example.test/mcp"}
            },
            "skills": {"paths": ["/user/skills"]}
        });
        let selected = Map::from_iter([(
            "brain_ws_current".to_owned(),
            serde_json::json!({"type": "local", "command": ["current"]}),
        )]);

        let merged = merge(
            inherited.as_object().expect("config object").clone(),
            "trusted prompt",
            Some(selected),
            Some((Path::new("/brain/skills"), &["todo"])),
        )
        .expect("merged config");
        let value: Value = serde_json::from_str(&merged).expect("merged JSON");

        assert_eq!(value["theme"], "catppuccin");
        assert_eq!(value["agent"]["review"]["description"], "Keep me");
        assert_eq!(value["agent"]["brain"]["prompt"], "trusted prompt");
        assert!(value["agent"]["brain"].get("model").is_none());
        assert!(value["mcp"].get("brain_ws_stale").is_none());
        assert_eq!(
            value["mcp"]["user-server"]["url"],
            "https://user.example.test/mcp"
        );
        assert_eq!(value["mcp"]["brain_ws_current"]["command"][0], "current");
        assert_eq!(value["skills"]["paths"][0], "/user/skills");
        assert_eq!(value["skills"]["paths"][1], "/brain/skills");
    }

    #[test]
    fn malformed_or_structurally_invalid_inherited_config_is_rejected() {
        assert_eq!(
            parse(Some("{not-json")).expect_err("malformed config"),
            AgentError::Frontend(INVALID_CONFIG.to_owned())
        );
        let invalid = serde_json::json!({"skills": {"paths": "not-an-array"}});
        assert_eq!(
            merge(
                invalid.as_object().expect("config object").clone(),
                "prompt",
                None,
                Some((Path::new("/brain/skills"), &["todo"])),
            )
            .expect_err("invalid skills paths"),
            AgentError::Frontend(INVALID_CONFIG.to_owned())
        );
    }
}
