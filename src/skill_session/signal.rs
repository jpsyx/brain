//! The completion signal for a skill session.
//!
//! A skill session runs in a separate, untracked brain-panel tab. Because that
//! session can involve long back-and-forth with the user, "the agent stopped
//! talking" is *not* a reliable completion signal. Instead the run, once it is
//! truly done, POSTs to its ingress-scoped local brain route with the one-time
//! token brain handed it in [`super::prompt::TOKEN_ENV`]. Brain appends that
//! instruction to the prompt it launches (see [`super::prompt`]), so a session
//! can signal completion without the skill it runs knowing anything about brain.
//!
//! **This module knows nothing about what a run produces.** The POST carries a
//! `require` list of paths: whatever outputs *this run* declared must exist
//! before its tab may close. Core supplies none; the list is empty unless the run
//! was told otherwise. The gate ([`ready_to_close`]) simply waits until every
//! listed path exists — an empty list closes immediately.
//!
//! The brain server and the TUI are separate processes, so the signal crosses
//! the boundary on disk, one file per token: the server writes [`done_path`]
//! (`record_done`); the TUI polls each open tab's token every event-loop tick
//! (`read_signal`) and, when the signal for that tab arrives *and* every required
//! output exists, auto-closes it. Per-token files are what let several skill
//! sessions run at once without one's signal closing another's tab, and the token
//! also guards against a stale signal from an earlier run closing a freshly
//! opened tab.
//!
//! The parsing and the close gate are pure (unit-tested); the file IO is a thin
//! shell around them.

use std::path::PathBuf;

use serde_json::json;

/// A parsed completion signal: the one-time token, plus the paths this run
/// declared must exist before its tab may close (empty when none were declared).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    pub token: String,
    pub require: Vec<String>,
}

/// Where one workspace's skill-session signals land in its UUID-scoped cache.
#[must_use]
pub fn done_dir(workspace: &crate::workspace::WorkspaceContext) -> PathBuf {
    workspace.paths().cache_dir().join("skill-sessions")
}

/// Where one session's completion signal lands. `None` for a token that isn't a
/// safe file name — the token arrives in an HTTP body, so it never becomes a
/// path before [`is_token_safe`] has vouched for it.
#[must_use]
pub fn done_path(workspace: &crate::workspace::WorkspaceContext, token: &str) -> Option<PathBuf> {
    is_token_safe(token).then(|| done_dir(workspace).join(format!("{token}.json")))
}

/// Whether a token may be used as a file name.
///
/// Non-empty, and nothing but unreserved characters. brain's own tokens are
/// UUIDs; this rejects anything that could escape the signal directory (`..`,
/// `/`) or surprise the file system, since the value comes from a request body.
#[must_use]
pub fn is_token_safe(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 128
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// Parse a `{"token": "...", "require": [...]}` completion-signal document.
///
/// Pure, so both the POST body parser and the on-disk signal reader share it.
/// Returns `None` for invalid JSON, a missing/non-string/empty token, or a token
/// that isn't safe as a file name. The `require` field is optional: absent,
/// non-array, and non-string/blank entries all collapse to (or drop from) an
/// empty-by-default list of required output paths.
#[must_use]
pub fn parse_signal(raw: &str) -> Option<Signal> {
    let value: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;
    let token = value.get("token")?.as_str()?.trim();
    if !is_token_safe(token) {
        return None;
    }
    let require = value
        .get("require")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some(Signal {
        token: token.to_owned(),
        require,
    })
}

/// Extract just the `token` field. Thin wrapper over [`parse_signal`].
#[must_use]
pub fn parse_token(raw: &str) -> Option<String> {
    parse_signal(raw).map(|s| s.token)
}

/// Whether a token-matched completion signal may close the tab: every path the
/// run declared as a required output must exist. `exists` reports whether a
/// given path is present on disk.
///
/// Pure (the disk probe is injected), and deliberately ignorant of *what* the
/// paths are — core assumes nothing about what a run produces. An empty
/// `require` list closes immediately: the no-op default when the run declared no
/// output.
#[must_use]
pub fn ready_to_close<F: Fn(&str) -> bool>(require: &[String], exists: F) -> bool {
    require.iter().all(|p| exists(p))
}

/// Record a completed skill-session run by writing its signal file.
///
/// Called by the brain server's session-done handler. `require` is the set of
/// output paths the run declared must exist before its tab closes (empty when
/// none). The `at` epoch is diagnostic only; the TUI matches on the token and
/// gates on `require`.
///
/// # Errors
/// Propagates any error creating the cache directory or writing the file, and
/// rejects a token that is not safe as a file name.
pub fn record_done(
    workspace: &crate::workspace::WorkspaceContext,
    token: &str,
    require: &[String],
) -> std::io::Result<()> {
    let path = done_path(workspace, token).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "skill-session token is not a valid signal name",
        )
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    std::fs::write(
        &path,
        json!({ "token": token, "require": require, "at": at }).to_string(),
    )
}

/// Read one session's pending completion signal, if any.
///
/// `None` when the file is absent or unparseable, or when the stored token
/// somehow differs from the one asked for. Impure (the disk read); the parse is
/// [`parse_signal`].
#[must_use]
pub fn read_signal(workspace: &crate::workspace::WorkspaceContext, token: &str) -> Option<Signal> {
    let raw = std::fs::read_to_string(done_path(workspace, token)?).ok()?;
    parse_signal(&raw).filter(|signal| signal.token == token)
}

/// Remove one session's signal file so a consumed (or superseded) signal cannot
/// fire again. Missing file is not an error.
pub fn clear(workspace: &crate::workspace::WorkspaceContext, token: &str) {
    if let Some(path) = done_path(workspace, token) {
        let _ = std::fs::remove_file(path);
    }
}

/// Drop every pending signal for this workspace. Called when the shell starts,
/// so a signal left behind by a crashed run can't close a tab opened later.
pub fn clear_all(workspace: &crate::workspace::WorkspaceContext) {
    let _ = std::fs::remove_dir_all(done_dir(workspace));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_token_reads_a_valid_body() {
        assert_eq!(
            parse_token(r#"{"token": "abc-123"}"#).as_deref(),
            Some("abc-123")
        );
    }

    #[test]
    fn parse_token_reads_the_on_disk_signal_shape() {
        // The file record carries an extra `at`; the same parser must read it.
        assert_eq!(
            parse_token(r#"{"token":"tok","at":1730000000}"#).as_deref(),
            Some("tok")
        );
    }

    #[test]
    fn parse_token_rejects_invalid_json() {
        assert!(parse_token("not json").is_none());
    }

    #[test]
    fn parse_token_rejects_missing_or_empty_token() {
        assert!(parse_token("{}").is_none());
        assert!(parse_token(r#"{"token": "   "}"#).is_none());
        assert!(parse_token(r#"{"token": 7}"#).is_none());
    }

    #[test]
    fn a_token_that_could_escape_the_signal_directory_is_rejected() {
        // The token arrives in a request body and becomes a file name, so a
        // traversal attempt must never reach the file system.
        assert!(!is_token_safe("../../etc/passwd"));
        assert!(!is_token_safe("a/b"));
        assert!(!is_token_safe(".."));
        assert!(!is_token_safe(""));
        assert!(parse_signal(r#"{"token":"../../evil"}"#).is_none());
        assert!(is_token_safe("2f6d0e64-3f24-4b0f-9a1e-2c9a1f1f0f11"));
    }

    #[test]
    fn done_path_lives_under_the_brain_cache_named_for_the_session() {
        let temporary = tempfile::tempdir().unwrap();
        let selected = workspace(
            temporary.path(),
            "personal",
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        );
        let path = done_path(&selected, "tok-1").expect("safe token");
        assert!(path.ends_with("tok-1.json"));
        assert!(
            path.to_string_lossy()
                .contains(selected.id().to_string().as_str())
        );
        assert!(done_path(&selected, "../evil").is_none());
    }

    #[test]
    fn parse_signal_reads_token_and_required_outputs() {
        let sig = parse_signal(r#"{"token":"t","require":["/a","/b"]}"#).unwrap();
        assert_eq!(sig.token, "t");
        assert_eq!(sig.require, vec!["/a".to_owned(), "/b".to_owned()]);
    }

    #[test]
    fn parse_signal_defaults_require_to_empty_when_absent() {
        // The no-op default: a run that declared no required outputs parses to
        // an empty list, so the tab closes as soon as the signal lands.
        let sig = parse_signal(r#"{"token":"t"}"#).unwrap();
        assert!(sig.require.is_empty());
    }

    #[test]
    fn parse_signal_drops_non_string_and_blank_require_entries() {
        let sig = parse_signal(r#"{"token":"t","require":["/a", 7, "   ", "/b"]}"#).unwrap();
        assert_eq!(sig.require, vec!["/a".to_owned(), "/b".to_owned()]);
    }

    #[test]
    fn parse_signal_rejects_missing_or_empty_token() {
        assert!(parse_signal(r#"{"require":["/a"]}"#).is_none());
        assert!(parse_signal("{}").is_none());
        assert!(parse_signal(r#"{"token":"  "}"#).is_none());
    }

    #[test]
    fn ready_to_close_when_no_outputs_declared() {
        // Empty require list ⇒ nothing to wait for ⇒ close immediately.
        assert!(ready_to_close(&[], |_| false));
    }

    #[test]
    fn ready_to_close_only_when_every_declared_output_exists() {
        let require = vec!["/a".to_owned(), "/b".to_owned()];
        assert!(ready_to_close(&require, |_| true));
        assert!(!ready_to_close(&require, |p| p == "/a"));
        assert!(!ready_to_close(&require, |_| false));
    }

    #[test]
    fn each_running_session_reads_only_its_own_signal() {
        // Two skill sessions can run at once, so one session's completion must
        // not read as another's.
        let temporary = tempfile::tempdir().unwrap();
        let selected = workspace(
            temporary.path(),
            "personal",
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        );

        record_done(&selected, "token-a", &[]).unwrap();

        assert_eq!(read_signal(&selected, "token-a").unwrap().token, "token-a");
        assert!(read_signal(&selected, "token-b").is_none());

        clear(&selected, "token-a");
        assert!(read_signal(&selected, "token-a").is_none());
    }

    #[test]
    fn signal_storage_is_scoped_to_the_resolved_workspace() {
        let temporary = tempfile::tempdir().unwrap();
        let personal = workspace(
            temporary.path(),
            "personal",
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        );
        let family = workspace(
            temporary.path(),
            "family",
            "e806258e-491a-436d-9db4-a5ca9903e0d4",
        );

        record_done(&family, "shared-token", &[]).unwrap();

        assert_eq!(
            read_signal(&family, "shared-token").unwrap().token,
            "shared-token"
        );
        assert!(read_signal(&personal, "shared-token").is_none());
        assert_ne!(
            done_path(&personal, "shared-token"),
            done_path(&family, "shared-token")
        );
    }

    #[test]
    fn clear_all_drops_every_pending_signal() {
        let temporary = tempfile::tempdir().unwrap();
        let selected = workspace(
            temporary.path(),
            "personal",
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        );
        record_done(&selected, "token-a", &[]).unwrap();
        record_done(&selected, "token-b", &[]).unwrap();

        clear_all(&selected);

        assert!(read_signal(&selected, "token-a").is_none());
        assert!(read_signal(&selected, "token-b").is_none());
    }

    fn workspace(
        home: &std::path::Path,
        name: &str,
        id: &str,
    ) -> crate::workspace::WorkspaceContext {
        crate::workspace::WorkspaceContext::new(
            home,
            crate::workspace::WorkspaceId::parse(id).unwrap(),
            crate::workspace::WorkspaceName::parse(name).unwrap(),
            &home.join(name),
            "tester",
            home,
        )
        .unwrap()
    }
}
