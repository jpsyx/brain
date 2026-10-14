//! Secret-free cancellation policy, with no executable invocation.

use super::model::{ExternalAction, LinkedIssue, OnRemove, Problem};
use super::plan::key;
use super::snapshot::Snapshot;
use crate::tasks::complete::field;
use std::collections::BTreeSet;

/// Plan grouped cancellations using canonical removed display identities.
pub(crate) fn plan(
    snapshot: &Snapshot,
    removed: &BTreeSet<String>,
    option: LinkedIssue,
) -> Result<Vec<ExternalAction>, Problem> {
    let policy = snapshot.integration.policy.as_ref();
    let cancel = match option {
        LinkedIssue::Keep => false,
        LinkedIssue::Cancel => true,
        LinkedIssue::Configured => {
            policy.is_some_and(|policy| policy.on_remove == OnRemove::Cancel)
        }
    };
    if !cancel {
        return Ok(Vec::new());
    }
    let rows: Vec<_> = snapshot
        .tasks
        .parsed
        .rows
        .iter()
        .chain(&snapshot.habits.parsed.rows)
        .collect();
    let issues: BTreeSet<_> = rows
        .iter()
        .filter(|row| removed.contains(&key(&field(row, "task_id"))))
        .map(|row| field(row, "linear_issue").trim().to_owned())
        .filter(|issue| !issue.is_empty())
        .collect();
    if issues.is_empty() {
        return Ok(Vec::new());
    }
    let policy = policy
        .filter(|policy| !policy.adapter.is_empty() && policy.link_field == "linear_issue")
        .filter(|_| snapshot.integration.adapter.is_some())
        .ok_or_else(|| {
            Problem::new(
                "adapter_unavailable",
                "canceling linked issues requires a configured available adapter",
                serde_json::json!({"issues":issues}),
            )
        })?;
    for row in &rows {
        let issue = field(row, "linear_issue").trim().to_owned();
        if !removed.contains(&key(&field(row, "task_id")))
            && field(row, "status").trim() != "done"
            && issues.contains(&issue)
        {
            return Err(Problem::new(
                "shared_external_link",
                "unfinished surviving row shares the linked issue",
                serde_json::json!({"task_id":field(row, "task_id"), "issue":issue}),
            ));
        }
    }
    issues
        .into_iter()
        .map(|issue| {
            let identity = serde_json::json!({
                "schema_version": 1,
                "workspace_id": snapshot.workspace_uuid,
                "adapter": policy.adapter,
                "issue": issue,
                "action": "cancel",
            });
            let action_id = super::fingerprint::digest(&identity).map_err(|error| {
                Problem::new(
                    "fingerprint_failed",
                    &error.to_string(),
                    serde_json::json!({}),
                )
            })?;
            Ok(ExternalAction {
                adapter: policy.adapter.clone(),
                issue,
                action_id,
            })
        })
        .collect()
}
