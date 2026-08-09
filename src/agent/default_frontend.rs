//! Which frontend the brain panel launches when no selector flag is passed.
//!
//! A machine-level choice: `default_agent_frontend` in the selected workspace's
//! brain env (a machine that has only one frontend installed must not be dragged
//! onto another by a different machine). The pure decisions — parsing the stored
//! name and letting an explicit `--claude` / `--codex` / `--open-code` win —
//! live here; the caller supplies the stored string.

use super::AgentKind;

/// The brain-env variable that names the fallback frontend.
pub const ENV_VAR: &str = "default_agent_frontend";

/// The frontend brain launches when nothing selects one.
pub const DEFAULT: &str = AgentKind::Claude.as_str();

/// Parse a stored frontend name.
///
/// Case- and whitespace-insensitive, and tolerant of the hyphenated
/// `open-code` spelling the CLI flag uses, since a user who typed
/// `--open-code` will reasonably write `open-code` here too.
#[must_use]
pub fn parse(raw: &str) -> Option<AgentKind> {
    match raw
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_'], "")
        .as_str()
    {
        "claude" => Some(AgentKind::Claude),
        "codex" => Some(AgentKind::Codex),
        "opencode" => Some(AgentKind::OpenCode),
        _ => None,
    }
}

/// Canonicalize a stored frontend name, or report it as unusable.
///
/// `brain env set default_agent_frontend=open-code` stores `opencode`, so the
/// value a reader sees is always one of [`AgentKind::as_str`]'s outputs.
pub fn canonicalize(raw: &str) -> Result<&'static str, InvalidFrontend> {
    parse(raw)
        .map(AgentKind::as_str)
        .ok_or_else(|| InvalidFrontend {
            value: raw.trim().to_owned(),
        })
}

/// A `default_agent_frontend` value that names no known frontend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidFrontend {
    value: String,
}

impl std::fmt::Display for InvalidFrontend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{ENV_VAR} must be one of claude, codex, opencode (got `{}`)",
            self.value
        )
    }
}

impl std::error::Error for InvalidFrontend {}

/// The frontend to launch: an explicit selector flag, else the configured
/// default, else Claude.
///
/// An unparseable stored value falls back to Claude rather than failing the
/// command: a typo in env must not make every `brain` invocation unusable.
#[must_use]
pub fn resolve(selected: Option<AgentKind>, configured: Option<&str>) -> AgentKind {
    selected
        .or_else(|| configured.and_then(parse))
        .unwrap_or(AgentKind::Claude)
}

/// The frontend one real invocation launches: the selector flag if present,
/// else the selected workspace's stored default, else Claude.
///
/// Thin shell over [`resolve`]: only a bootstrapped workspace has env to read,
/// so registry-only and no-workspace commands (which never open a brain panel)
/// fall back to the flag or Claude.
#[must_use]
pub fn resolved_frontend(
    selected: Option<AgentKind>,
    bootstrap: &crate::workspace::BootstrapContext,
) -> AgentKind {
    let configured = match bootstrap {
        crate::workspace::BootstrapContext::Ready(command) => {
            crate::env::resolve_one(command, ENV_VAR)
        }
        crate::workspace::BootstrapContext::None
        | crate::workspace::BootstrapContext::RegistryOnly(_) => None,
    };
    resolve(selected, configured.as_deref())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use serde_json::{Map, json};

    use super::*;
    use crate::workspace::{
        BootstrapContext, CommandContext, MachineRegistry, RegistryStore, WorkspaceContext,
        WorkspaceId, WorkspaceName, WorkspaceRecord,
    };

    /// A ready bootstrap over a real registry file whose selected record carries
    /// `env`.
    fn ready(env: Map<String, serde_json::Value>) -> (tempfile::TempDir, BootstrapContext) {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("brain");
        std::fs::create_dir_all(&root).unwrap();
        let name = WorkspaceName::parse("brain").unwrap();
        let id = WorkspaceId::parse("6f1a0d2c-2f2e-4a8f-9d0e-0c3b2a1d4e5f").unwrap();
        let registry = MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: name.clone(),
            workspaces: BTreeMap::from([(
                name.clone(),
                WorkspaceRecord {
                    workspace_id: id,
                    root: root.clone(),
                    aliases: BTreeSet::new(),
                    local_user_id: "pablo".to_owned(),
                    receiver_enabled: false,
                    env,
                },
            )]),
            env: serde_json::Map::new(),
        };
        let store = RegistryStore::from_path(home.path().join("config/brain/env.json"));
        store.replace(&registry).unwrap();
        let context = CommandContext::for_test(
            Arc::new(
                WorkspaceContext::new(home.path(), id, name, &root, "pablo", home.path()).unwrap(),
            ),
            store,
            "pablo",
        );
        (home, BootstrapContext::Ready(context))
    }

    #[test]
    fn a_ready_workspace_launches_its_stored_default_frontend() {
        let (_home, bootstrap) = ready(Map::from_iter([(ENV_VAR.to_owned(), json!("codex"))]));

        assert_eq!(resolved_frontend(None, &bootstrap), AgentKind::Codex);
    }

    #[test]
    fn a_selector_flag_overrides_the_stored_default() {
        let (_home, bootstrap) = ready(Map::from_iter([(ENV_VAR.to_owned(), json!("codex"))]));

        assert_eq!(
            resolved_frontend(Some(AgentKind::Claude), &bootstrap),
            AgentKind::Claude
        );
    }

    #[test]
    fn an_unset_default_still_launches_claude() {
        let (_home, bootstrap) = ready(Map::new());

        assert_eq!(resolved_frontend(None, &bootstrap), AgentKind::Claude);
    }

    #[test]
    fn commands_with_no_ready_workspace_use_the_flag_or_claude() {
        // Registry-only and no-workspace commands never open a brain panel, but
        // dispatch still needs a value; reading env would need a workspace.
        assert_eq!(
            resolved_frontend(None, &BootstrapContext::None),
            AgentKind::Claude
        );
        assert_eq!(
            resolved_frontend(Some(AgentKind::Codex), &BootstrapContext::None),
            AgentKind::Codex
        );
    }

    #[test]
    fn every_frontend_name_round_trips_through_its_stable_string() {
        for kind in AgentKind::ALL {
            assert_eq!(parse(kind.as_str()), Some(kind), "{kind:?}");
        }
    }

    #[test]
    fn stored_names_tolerate_case_padding_and_the_hyphenated_opencode_spelling() {
        assert_eq!(parse("  Codex "), Some(AgentKind::Codex));
        assert_eq!(parse("OpenCode"), Some(AgentKind::OpenCode));
        // The CLI flag is `--open-code`, so accept that spelling here too.
        assert_eq!(parse("open-code"), Some(AgentKind::OpenCode));
        assert_eq!(parse("open_code"), Some(AgentKind::OpenCode));
    }

    #[test]
    fn an_unknown_name_is_not_a_frontend() {
        assert_eq!(parse("gemini"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn canonicalize_normalizes_to_the_stable_string_and_names_the_valid_set() {
        assert_eq!(canonicalize("open-code"), Ok("opencode"));
        assert_eq!(canonicalize(" CLAUDE "), Ok("claude"));
        let error = canonicalize("gemini").unwrap_err().to_string();
        assert!(error.contains("claude, codex, opencode"), "{error}");
        assert!(error.contains("gemini"), "{error}");
    }

    #[test]
    fn an_explicit_selector_beats_the_configured_default() {
        assert_eq!(
            resolve(Some(AgentKind::Claude), Some("codex")),
            AgentKind::Claude
        );
        assert_eq!(
            resolve(Some(AgentKind::OpenCode), Some("claude")),
            AgentKind::OpenCode
        );
    }

    #[test]
    fn with_no_selector_the_configured_default_decides() {
        assert_eq!(resolve(None, Some("codex")), AgentKind::Codex);
        assert_eq!(resolve(None, Some("open-code")), AgentKind::OpenCode);
    }

    #[test]
    fn claude_is_the_fallback_when_nothing_usable_is_configured() {
        assert_eq!(resolve(None, None), AgentKind::Claude);
        assert_eq!(resolve(None, Some("")), AgentKind::Claude);
        // A typo must not make every invocation fail.
        assert_eq!(resolve(None, Some("gemini")), AgentKind::Claude);
    }
}
