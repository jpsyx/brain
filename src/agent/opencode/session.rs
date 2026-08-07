//! Read-only OpenCode session discovery scoped to one selected workspace.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::agent::{AgentError, AgentSession};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SessionSnapshot {
    ids: HashSet<String>,
}

impl SessionSnapshot {
    pub(super) fn contains(&self, session: &AgentSession) -> bool {
        self.ids.contains(session.as_str())
    }
}

#[derive(Debug, Deserialize)]
struct ListedSession {
    id: String,
    directory: PathBuf,
    #[serde(default, rename = "parentID", alias = "parentId", alias = "parent_id")]
    parent_id: Option<String>,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    deleted: bool,
}

pub(super) fn discover(
    command: &str,
    workspace_root: &Path,
) -> Result<SessionSnapshot, AgentError> {
    let output = super::probe::read_only_output(
        command,
        &["session", "list", "--format", "json"],
        workspace_root,
        "session discovery",
    )?;
    parse(&output, workspace_root)
}

fn parse(output: &str, workspace_root: &Path) -> Result<SessionSnapshot, AgentError> {
    if output.trim().is_empty() {
        return Ok(SessionSnapshot::default());
    }
    let sessions: Vec<ListedSession> = serde_json::from_str(output).map_err(|_| {
        AgentError::Frontend(
            "OpenCode session discovery returned malformed JSON; run `opencode session list --format json` in the selected workspace".to_owned(),
        )
    })?;
    if sessions.iter().any(|session| session.id.trim().is_empty()) {
        return Err(AgentError::Frontend(
            "OpenCode session discovery returned a blank session ID".to_owned(),
        ));
    }
    let workspace_root = normalize(workspace_root);
    let ids = sessions
        .into_iter()
        .filter(|session| {
            session.parent_id.is_none()
                && !session.archived
                && !session.deleted
                && normalize(&session.directory) == workspace_root
        })
        .map(|session| session.id)
        .collect();
    Ok(SessionSnapshot { ids })
}

fn normalize(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_unique_live_root_sessions_for_the_exact_workspace() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let other = temporary.path().join("other");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let output = serde_json::json!([
            {"id":"root","directory":workspace,"unknown":"accepted"},
            {"id":"root","directory":workspace},
            {"id":"child","directory":workspace,"parentID":"root"},
            {"id":"wrong","directory":other},
            {"id":"archived","directory":workspace,"archived":true},
            {"id":"deleted","directory":workspace,"deleted":true}
        ])
        .to_string();

        let snapshot = parse(&output, &workspace).unwrap();

        assert!(snapshot.contains(&AgentSession::new("root").unwrap()));
        for missing in ["child", "wrong", "archived", "deleted"] {
            assert!(!snapshot.contains(&AgentSession::new(missing).unwrap()));
        }
        assert_eq!(snapshot.ids.len(), 1);
    }

    #[test]
    fn accepts_blank_cli_output_and_an_explicit_empty_array() {
        let workspace = Path::new("/workspace");
        assert_eq!(parse("", workspace).unwrap(), SessionSnapshot::default());
        assert_eq!(parse("[]", workspace).unwrap(), SessionSnapshot::default());
    }

    #[test]
    fn malformed_json_and_missing_required_fields_are_actionable_errors() {
        for output in [
            "not json",
            r#"[{"directory":"/workspace"}]"#,
            r#"[{"id":"session"}]"#,
        ] {
            let error = parse(output, Path::new("/workspace")).unwrap_err();
            assert!(error.to_string().contains("malformed JSON"), "{error}");
        }
    }
}
