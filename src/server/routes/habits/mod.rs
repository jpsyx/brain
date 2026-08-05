//! The ingress-scoped habits page and mark-done endpoint.
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

use chrono::Local;
use serde_json::json;

use crate::tasks::complete::{complete_in_root_protected_with_owner_and_today, normalize_id};

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

/// Render today's habits page for one already-resolved workspace.
#[must_use]
pub fn page(
    workspace: &crate::workspace::WorkspaceContext,
    ingress: crate::server::IngressId,
) -> String {
    let today = Local::now().date_naive();
    let rows = model::load(workspace.root());
    let (pending, completed) = model::classify(rows, today);
    view::render(&pending, &completed, today, ingress)
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
pub fn done(workspace: &crate::workspace::WorkspaceContext, body: &str) -> DoneOutcome {
    let raw_id = match parse_task_id(body) {
        Ok(id) => id,
        Err(bad) => return bad,
    };
    let id = match normalize_id(&raw_id) {
        Ok(id) => id,
        Err(e) => return DoneOutcome::BadRequest(e.to_string()),
    };
    let lock_path = workspace.paths().task_store_lock();
    let owner = match crate::tasks::store_lock::TaskStoreOwner::acquire_path(&lock_path) {
        Ok(owner) => owner,
        Err(error) => return DoneOutcome::Failed(error.to_string()),
    };
    let enabled = match crate::config::Config::try_load_from_root(workspace.root()) {
        Ok(config) => config.enable_triage_habits,
        Err(error) => return DoneOutcome::Failed(error.to_string()),
    };
    match complete_in_root_protected_with_owner_and_today(
        workspace.root(),
        &lock_path,
        &owner,
        &id,
        Local::now().date_naive(),
        enabled,
    ) {
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

    #[test]
    fn web_completion_rejects_managed_triage_rows() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("family");
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(root.join(".config")).unwrap();
        std::fs::write(
            root.join("tasks/tasks.csv"),
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tasks/habits.csv"),
            "task_uuid,task_id,task_name,status,due_date,assigned_to,recur_interval,recur_unit,system_key\n\
             8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,H1,Morning Triage,not_started,2026-08-03,member,1,days,brain.triage.daily\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".config/config.json"),
            "{\"enable_triage_habits\":true}\n",
        )
        .unwrap();
        let workspace = crate::workspace::WorkspaceContext::new(
            temporary.path(),
            crate::workspace::WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap(),
            crate::workspace::WorkspaceName::parse("family").unwrap(),
            &root,
            "member",
            temporary.path(),
        )
        .unwrap();

        let outcome = done(&workspace, r#"{"task_id":"H1"}"#);

        assert!(matches!(
            outcome,
            DoneOutcome::Failed(message) if message.contains("cannot be completed outside triage")
        ));
    }

    #[test]
    fn web_completion_reads_portable_config_after_acquiring_task_store_ownership() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("family");
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(root.join(".config")).unwrap();
        std::fs::write(
            root.join("tasks/tasks.csv"),
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        std::fs::write(
            root.join("tasks/habits.csv"),
            "task_uuid,task_id,task_name,status,due_date,assigned_to,recur_interval,recur_unit,system_key\n\
             8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,H1,Morning Triage,not_started,2026-08-03,member,1,days,brain.triage.daily\n",
        )
        .unwrap();
        let config_path = root.join(".config/config.json");
        std::fs::write(&config_path, "{\"enable_triage_habits\":true}\n").unwrap();
        let workspace = crate::workspace::WorkspaceContext::new(
            temporary.path(),
            crate::workspace::WorkspaceId::parse("d61501ea-48b6-4cd4-a472-b69bcac74f17").unwrap(),
            crate::workspace::WorkspaceName::parse("family").unwrap(),
            &root,
            "member",
            temporary.path(),
        )
        .unwrap();
        let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(&workspace).unwrap();
        let request_workspace = workspace;
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let request = std::thread::spawn(move || {
            done_tx
                .send(done(&request_workspace, r#"{"task_id":"H1"}"#))
                .unwrap();
        });

        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "request should wait for task-store ownership"
        );
        std::fs::write(&config_path, "{\"enable_triage_habits\":false}\n").unwrap();
        drop(owner);

        let outcome = done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        request.join().unwrap();
        assert!(matches!(outcome, DoneOutcome::Done { .. }));
    }

    #[test]
    fn web_completion_rejects_malformed_portable_config_without_mutating_rows() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("family");
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(root.join(".config")).unwrap();
        std::fs::write(
            root.join("tasks/tasks.csv"),
            "task_uuid,task_id,task_name,status,assigned_to,system_key\n",
        )
        .unwrap();
        let habits_path = root.join("tasks/habits.csv");
        std::fs::write(
            &habits_path,
            "task_uuid,task_id,task_name,status,due_date,assigned_to,recur_interval,recur_unit,system_key\n\
             8f4ff482-4d40-4a2d-91b1-73ca9f1bfad4,H1,Floss,not_started,2026-08-03,member,1,days,\n",
        )
        .unwrap();
        std::fs::write(root.join(".config/config.json"), "not json\n").unwrap();
        let before = std::fs::read(&habits_path).unwrap();
        let workspace = crate::workspace::WorkspaceContext::new(
            temporary.path(),
            crate::workspace::WorkspaceId::new(),
            crate::workspace::WorkspaceName::parse("family").unwrap(),
            &root,
            "member",
            temporary.path(),
        )
        .unwrap();

        let outcome = done(&workspace, r#"{"task_id":"H1"}"#);

        assert!(matches!(outcome, DoneOutcome::Failed(_)));
        assert_eq!(std::fs::read(habits_path).unwrap(), before);
    }
}
