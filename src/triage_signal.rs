//! The completion signal for the ephemeral daily-triage session.
//!
//! Daily triage runs in a separate, untracked brain-panel tab (see the tasks
//! view's triage tab). Because that agent session can involve long
//! back-and-forth with the user, "the LLM stopped talking" is *not* a reliable
//! completion signal. Instead the `/triage` skill, once the pass is truly done
//! (the PDF is written, the Morning Triage habit marked), POSTs to the local
//! brain server's [`DONE_PATH`] with the one-time token brain handed it in
//! `BRAIN_TRIAGE_TOKEN`.
//!
//! The brain server and the TUI are separate processes, so the signal crosses
//! the boundary on disk: the server writes [`done_path`] (`record_done`); the
//! TUI polls it each event-loop tick (`read_token`) and, when the token matches
//! the tab it opened, auto-closes the triage tab. The token guards against a
//! stale signal from an earlier run closing a freshly-opened tab.
//!
//! The parsing is pure (unit-tested); the file IO is a thin shell around it.

use std::path::PathBuf;

use serde_json::json;

/// The brain-server path the `/triage` skill POSTs to when a daily-triage pass
/// completes. brain hands the full URL to the session in `BRAIN_TRIAGE_DONE_URL`.
pub const DONE_PATH: &str = "/triage/done";

/// Where the completion signal lands: `~/.cache/brain/triage-done.json`.
/// Mirrors [`crate::state::Db::default_path`] and the server record.
#[must_use]
pub fn done_path() -> PathBuf {
    let base = std::env::var_os("HOME").map_or_else(
        || PathBuf::from("."),
        |home| PathBuf::from(home).join(".cache").join("brain"),
    );
    base.join("triage-done.json")
}

/// Extract the `token` field from a `{"token": "..."}` JSON document.
///
/// Pure, so both the POST body parser and the on-disk signal reader share it.
/// Returns `None` for invalid JSON, a missing/non-string token, or an empty
/// token.
#[must_use]
pub fn parse_token(raw: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;
    let token = value.get("token")?.as_str()?.trim();
    (!token.is_empty()).then(|| token.to_owned())
}

/// Record a completed daily-triage pass for `token` by writing the signal file.
/// Called by the brain server's `POST /triage/done` handler. The `at` epoch is
/// diagnostic only; the TUI matches on the token.
///
/// # Errors
/// Propagates any error creating the cache directory or writing the file.
pub fn record_done(token: &str) -> std::io::Result<()> {
    let path = done_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    std::fs::write(&path, json!({ "token": token, "at": at }).to_string())
}

/// Read the pending completion token from the signal file, if any. `None` when
/// the file is absent or unparseable. Impure (the disk read); the parse is
/// [`parse_token`].
#[must_use]
pub fn read_token() -> Option<String> {
    let raw = std::fs::read_to_string(done_path()).ok()?;
    parse_token(&raw)
}

/// Remove the signal file so a consumed (or superseded) signal cannot fire
/// again. Missing file is not an error.
pub fn clear() {
    let _ = std::fs::remove_file(done_path());
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
        let path = done_path();
        assert!(path.ends_with("triage-done.json"));
    }
}
