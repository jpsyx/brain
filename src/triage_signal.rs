//! The completion signal for the ephemeral daily-triage session.
//!
//! Daily triage runs in a separate, untracked brain-panel tab (see the tasks
//! view's triage tab). Because that agent session can involve long
//! back-and-forth with the user, "the LLM stopped talking" is *not* a reliable
//! completion signal. Instead the `/triage` skill, once the pass is truly done
//! (the Morning Triage habit marked and every output the run *declared it must
//! produce* actually on disk), POSTs to its ingress-scoped local brain route
//! with the one-time token brain handed it in `BRAIN_TRIAGE_TOKEN`.
//!
//! **This module knows nothing about what a triage pass produces.** The core
//! `/triage` skill has hooks (`<!-- brain:ext … -->`) a user's extension *may*
//! fill with extra work — an inbox sweep, a report, a printable — but core
//! cannot assume any such extension exists, nor that any particular file is
//! written. So the completion POST carries a `require` list of paths: whatever
//! outputs *this run* (core plus whatever extensions were rendered in) declared
//! must exist before the tab may close. Core supplies none; the list is empty
//! unless an extension contributed one. The gate ([`ready_to_close`]) simply
//! waits until every listed path exists — an empty list closes immediately, so
//! a fork with no extensions behaves exactly as before.
//!
//! The brain server and the TUI are separate processes, so the signal crosses
//! the boundary on disk: the server writes [`done_path`] (`record_done`); the
//! TUI polls it each event-loop tick (`read_signal`) and, when the token
//! matches the tab it opened *and* every required output exists, auto-closes
//! the triage tab. The token guards against a stale signal from an earlier run
//! closing a freshly-opened tab; the `require` gate guards against a premature
//! signal closing the tab before the run's declared outputs are on disk.
//!
//! The parsing and the close gate are pure (unit-tested); the file IO is a thin
//! shell around them.

use std::path::PathBuf;

use serde_json::json;

/// A parsed completion signal: the one-time token, plus the paths this run
/// declared must exist before the triage tab may close (empty when no
/// extension contributed any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    pub token: String,
    pub require: Vec<String>,
}

/// Where one workspace's completion signal lands in its UUID-scoped cache.
#[must_use]
pub fn done_path(workspace: &crate::workspace::WorkspaceContext) -> PathBuf {
    workspace.paths().cache_dir().join("triage-done.json")
}

/// Parse a `{"token": "...", "require": [...]}` completion-signal document.
///
/// Pure, so both the POST body parser and the on-disk signal reader share it.
/// Returns `None` for invalid JSON or a missing/non-string/empty token. The
/// `require` field is optional: absent, non-array, and non-string/blank entries
/// all collapse to (or drop from) an empty-by-default list of required output
/// paths. Core declares no paths, so the empty default is the no-extension
/// case.
#[must_use]
pub fn parse_signal(raw: &str) -> Option<Signal> {
    let value: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;
    let token = value.get("token")?.as_str()?.trim();
    if token.is_empty() {
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
/// paths are — core assumes nothing about extension-produced artifacts. An
/// empty `require` list closes immediately: the no-op default when no extension
/// declared any output.
#[must_use]
pub fn ready_to_close<F: Fn(&str) -> bool>(require: &[String], exists: F) -> bool {
    require.iter().all(|p| exists(p))
}

/// Record a completed daily-triage pass by writing the signal file.
///
/// Called by the brain server's `POST /triage/done` handler. `require` is the
/// set of output paths the run declared must exist before the tab closes (empty
/// when none). The `at` epoch is diagnostic only; the TUI matches on the token
/// and gates on `require`.
///
/// # Errors
/// Propagates any error creating the cache directory or writing the file.
pub fn record_done(
    workspace: &crate::workspace::WorkspaceContext,
    token: &str,
    require: &[String],
) -> std::io::Result<()> {
    let path = done_path(workspace);
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

/// Read the pending completion signal from the signal file, if any. `None` when
/// the file is absent or unparseable. Impure (the disk read); the parse is
/// [`parse_signal`].
#[must_use]
pub fn read_signal(workspace: &crate::workspace::WorkspaceContext) -> Option<Signal> {
    let raw = std::fs::read_to_string(done_path(workspace)).ok()?;
    parse_signal(&raw)
}

/// Remove the signal file so a consumed (or superseded) signal cannot fire
/// again. Missing file is not an error.
pub fn clear(workspace: &crate::workspace::WorkspaceContext) {
    let _ = std::fs::remove_file(done_path(workspace));
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
    fn done_path_lives_under_the_brain_cache() {
        let temporary = tempfile::tempdir().unwrap();
        let selected = workspace(
            temporary.path(),
            "personal",
            "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b",
        );
        let path = done_path(&selected);
        assert!(path.ends_with("triage-done.json"));
        assert!(
            path.to_string_lossy()
                .contains(selected.id().to_string().as_str())
        );
    }

    #[test]
    fn parse_signal_reads_token_and_required_outputs() {
        let sig = parse_signal(r#"{"token":"t","require":["/a","/b"]}"#).unwrap();
        assert_eq!(sig.token, "t");
        assert_eq!(sig.require, vec!["/a".to_owned(), "/b".to_owned()]);
    }

    #[test]
    fn parse_signal_defaults_require_to_empty_when_absent() {
        // The no-op default: a run that declared no required outputs (e.g. a
        // fork with no extension) parses to an empty list, so the tab closes
        // on signal exactly as before.
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

        record_done(&family, "family-token", &[]).unwrap();

        assert_eq!(read_signal(&family).unwrap().token, "family-token");
        assert!(read_signal(&personal).is_none());
        assert_ne!(done_path(&personal), done_path(&family));
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
