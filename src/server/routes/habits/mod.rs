//! The `/habits` route: a today's-habits page and a mark-done endpoint.
//!
//! Thin controller. [`model`] reads + filters + sorts habits (pure decisions
//! plus one file read); [`view`] renders them into the `web/habits/` shell.
//! Marking done reuses brain's native completion machinery
//! ([`crate::tasks::complete`]), so the web "done" is exactly
//! `brain tasks complete` — including habit recurrence — rather than a
//! reimplementation. The pure decisions ([`parse_task_id`],
//! [`DoneOutcome::response`]) are split out from the mutation call so they are
//! unit-testable without touching disk.

pub mod model;
pub mod view;

use std::path::Path;

use chrono::Local;
use serde_json::json;

use crate::tasks::complete::{complete_in_root, normalize_id};

/// The result of a `POST /habits/done`, ready to become an HTTP response.
#[derive(Debug, PartialEq, Eq)]
pub enum DoneOutcome {
    /// The habit was marked done; `next_due` is the spawned next occurrence,
    /// if the completion machinery reported one.
    Done { next_due: Option<String> },
    /// The request body was missing/invalid (→ HTTP 400).
    BadRequest(String),
    /// Completion itself failed (→ HTTP 500).
    Failed(String),
}

impl DoneOutcome {
    /// Map the outcome to an HTTP `(status, json_body)` pair. Pure.
    #[must_use]
    pub fn response(&self) -> (u16, String) {
        match self {
            Self::Done { next_due } => {
                (200, json!({ "ok": true, "next_due": next_due }).to_string())
            }
            Self::BadRequest(msg) => (400, json!({ "error": msg }).to_string()),
            Self::Failed(msg) => (500, json!({ "error": msg }).to_string()),
        }
    }
}

/// Render today's habits page for `root`, as of the local date.
#[must_use]
pub fn page(root: &Path) -> String {
    let today = Local::now().date_naive();
    let rows = model::load(root);
    let (pending, completed) = model::classify(rows, today);
    view::render(&pending, &completed, today)
}

/// Extract and validate the `task_id` from a `POST /habits/done` JSON body.
/// Pure: returns the id, or the [`DoneOutcome::BadRequest`] to send back.
fn parse_task_id(body: &str) -> Result<String, DoneOutcome> {
    let value: serde_json::Value = serde_json::from_str(body.trim())
        .map_err(|_| DoneOutcome::BadRequest("invalid json".to_owned()))?;
    let task_id = value
        .get("task_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim();
    if task_id.is_empty() {
        return Err(DoneOutcome::BadRequest("missing task_id".to_owned()));
    }
    Ok(task_id.to_owned())
}

/// Mark a habit done through brain's native completion path.
#[must_use]
pub fn done(root: &Path, body: &str) -> DoneOutcome {
    let raw_id = match parse_task_id(body) {
        Ok(id) => id,
        Err(bad) => return bad,
    };
    let id = match normalize_id(&raw_id) {
        Ok(id) => id,
        Err(e) => return DoneOutcome::BadRequest(e.to_string()),
    };
    match complete_in_root(root, &id) {
        Ok(result) => DoneOutcome::Done {
            next_due: result.next_due,
        },
        Err(e) => DoneOutcome::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_task_id_accepts_a_valid_body() {
        assert_eq!(parse_task_id(r#"{"task_id": "H7"}"#).unwrap(), "H7");
    }

    #[test]
    fn parse_task_id_rejects_invalid_json() {
        assert_eq!(
            parse_task_id("not json"),
            Err(DoneOutcome::BadRequest("invalid json".to_owned()))
        );
    }

    #[test]
    fn parse_task_id_rejects_missing_or_empty_id() {
        assert!(matches!(
            parse_task_id("{}"),
            Err(DoneOutcome::BadRequest(_))
        ));
        assert!(matches!(
            parse_task_id(r#"{"task_id": "   "}"#),
            Err(DoneOutcome::BadRequest(_))
        ));
    }

    #[test]
    fn response_maps_done_to_200_with_next_due() {
        let (status, body) = DoneOutcome::Done {
            next_due: Some("2026-08-01".to_owned()),
        }
        .response();
        assert_eq!(status, 200);
        assert!(body.contains(r#""ok":true"#));
        assert!(body.contains(r#""next_due":"2026-08-01""#));
    }

    #[test]
    fn response_maps_done_without_next_due_to_json_null() {
        let (status, body) = DoneOutcome::Done { next_due: None }.response();
        assert_eq!(status, 200);
        assert!(body.contains(r#""next_due":null"#));
    }

    #[test]
    fn response_maps_bad_request_to_400() {
        let (status, body) = DoneOutcome::BadRequest("missing task_id".to_owned()).response();
        assert_eq!(status, 400);
        assert!(body.contains(r#""error":"missing task_id""#));
    }

    #[test]
    fn response_maps_failed_to_500() {
        let (status, body) = DoneOutcome::Failed("boom".to_owned()).response();
        assert_eq!(status, 500);
        assert!(body.contains(r#""error":"boom""#));
    }
}
