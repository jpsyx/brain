//! Generic inbound webhook capture endpoint.

use std::path::Path;

use chrono::{Local, NaiveDateTime};
use serde_json::json;

/// Result of a webhook capture request.
#[derive(Debug, PartialEq, Eq)]
pub enum CaptureOutcome {
    Captured { path: String },
    BadRequest(String),
    Failed(String),
}

impl CaptureOutcome {
    /// Convert the outcome into an HTTP status code and JSON response body.
    #[must_use]
    pub fn response(self) -> (u16, String) {
        match self {
            Self::Captured { path } => (202, json!({ "ok": true, "path": path }).to_string()),
            Self::BadRequest(error) => (400, json!({ "ok": false, "error": error }).to_string()),
            Self::Failed(error) => (500, json!({ "ok": false, "error": error }).to_string()),
        }
    }
}

/// Capture an inbound webhook body using the current local clock.
#[must_use]
pub fn capture(root: &Path, body: &str) -> CaptureOutcome {
    capture_at(root, body, Local::now().naive_local())
}

fn capture_at(root: &Path, body: &str, now: NaiveDateTime) -> CaptureOutcome {
    if body.trim().is_empty() {
        return CaptureOutcome::BadRequest("request body is empty".to_owned());
    }
    let dir = root.join("scratch").join("webhooks");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return CaptureOutcome::Failed(format!("creating webhook capture directory: {e}"));
    }
    let Some((rel, path)) = next_capture_path(root, now, extension_for(body)) else {
        return CaptureOutcome::Failed("too many webhook captures in the same second".to_owned());
    };
    match std::fs::write(&path, body) {
        Ok(()) => CaptureOutcome::Captured { path: rel },
        Err(e) => CaptureOutcome::Failed(format!("writing webhook capture: {e}")),
    }
}

fn next_capture_path(
    root: &Path,
    now: NaiveDateTime,
    extension: &str,
) -> Option<(String, std::path::PathBuf)> {
    let stamp = now.format("%Y%m%dT%H%M%S");
    for n in 1..=9999 {
        let rel = format!("scratch/webhooks/{stamp}-{n:04}.{extension}");
        let path = root.join(&rel);
        if !path.exists() {
            return Some((rel, path));
        }
    }
    None
}

fn extension_for(body: &str) -> &'static str {
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        "json"
    } else {
        "txt"
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    #[test]
    fn capture_writes_json_payload_under_scratch_webhooks() {
        let root = tempfile::tempdir().unwrap();
        let now = NaiveDate::from_ymd_opt(2026, 7, 26)
            .unwrap()
            .and_hms_opt(9, 30, 4)
            .unwrap();

        let outcome = capture_at(root.path(), r#"{"title":"New thing"}"#, now);

        assert_eq!(
            outcome,
            CaptureOutcome::Captured {
                path: "scratch/webhooks/20260726T093004-0001.json".to_owned(),
            }
        );
        assert_eq!(
            std::fs::read_to_string(
                root.path()
                    .join("scratch/webhooks/20260726T093004-0001.json")
            )
            .unwrap(),
            r#"{"title":"New thing"}"#
        );
    }

    #[test]
    fn response_maps_capture_to_202_with_relative_path() {
        let (status, body) = CaptureOutcome::Captured {
            path: "scratch/webhooks/example.json".to_owned(),
        }
        .response();

        assert_eq!(status, 202);
        assert_eq!(
            body,
            r#"{"ok":true,"path":"scratch/webhooks/example.json"}"#
        );
    }

    #[test]
    fn empty_body_is_a_bad_request() {
        let root = tempfile::tempdir().unwrap();
        let now = NaiveDate::from_ymd_opt(2026, 7, 26)
            .unwrap()
            .and_hms_opt(9, 30, 4)
            .unwrap();

        let outcome = capture_at(root.path(), "   ", now);
        let (status, body) = outcome.response();

        assert_eq!(status, 400);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            json!({ "ok": false, "error": "request body is empty" })
        );
        assert!(!root.path().join("scratch/webhooks").exists());
    }

    #[test]
    fn capture_uses_the_next_sequence_when_a_timestamp_exists() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("scratch/webhooks");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("20260726T093004-0001.json"), "old").unwrap();
        let now = NaiveDate::from_ymd_opt(2026, 7, 26)
            .unwrap()
            .and_hms_opt(9, 30, 4)
            .unwrap();

        let outcome = capture_at(root.path(), r#"{"title":"New thing"}"#, now);

        assert_eq!(
            outcome,
            CaptureOutcome::Captured {
                path: "scratch/webhooks/20260726T093004-0002.json".to_owned(),
            }
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("20260726T093004-0002.json")).unwrap(),
            r#"{"title":"New thing"}"#
        );
    }
}
