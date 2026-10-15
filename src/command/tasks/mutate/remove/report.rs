//! Public removal envelopes deliberately exclude private snapshot/file bytes.
use serde_json::{Value, json};

use crate::tasks::removal::model::{Problem, RemovalPlan};
use crate::tasks::removal::operations::{Operation, Receipt};
use crate::theme::Theme;

pub(super) fn preview(plan: &RemovalPlan) -> Value {
    json!({
        "schema_version": 1, "kind": "preview", "fingerprint": plan.fingerprint,
        "ready": plan.blockers.is_empty(), "targets": plan.targets,
        "effects": {
            "updated": plan.edits.iter().map(|edit| edit.relative.to_string_lossy()).collect::<Vec<_>>(),
            "external": plan.external, "successors": plan.successors,
            "relationships": crate::tasks::removal::presentation::project(plan),
        },
        "blockers": plan.blockers, "warnings": plan.warnings,
        "options": plan.options,
        "available_options": {"dependents": ["reject", "unlink", "rewire"], "linked_issue": ["configured", "keep", "cancel"]},
    })
}
pub(super) fn result(receipt: &Receipt) -> Value {
    json!({
        "schema_version": 1, "kind": "result", "operation_id": receipt.operation_id,
        "status": if receipt.warnings.is_empty() { "committed" } else { "committed_with_warnings" },
        "committed": receipt.committed, "removed": receipt.removed, "updated": receipt.updated,
        "external": receipt.external, "warnings": receipt.warnings,
    })
}
pub(super) fn operation(operation: &Operation) -> Value {
    json!({
        "schema_version": 1, "kind": "operation", "operation_id": operation.operation_id,
        "phase": operation.phase, "fingerprint": operation.fingerprint,
        "targets": operation.plan.targets, "external": operation.external,
        "receipt": operation.receipt.as_ref().map(result),
    })
}
pub(super) fn error(problem: &Problem) -> Value {
    json!({"schema_version": 1, "kind": "error", "code": problem.code,
        "message": problem.message, "details": problem.details})
}
pub(super) fn human(value: &Value, theme: Theme) -> String {
    let mut lines = Vec::new();
    match value["kind"].as_str() {
        Some("preview") => {
            lines.push(theme.info(&format!(
                "Removal preview ({})",
                value["fingerprint"].as_str().unwrap_or_default()
            )));
            if let Some(targets) = value["targets"].as_array() {
                for target in targets {
                    lines.push(format!(
                        "  {}  {}",
                        target["task_id"].as_str().unwrap_or_default(),
                        target["task_name"].as_str().unwrap_or_default()
                    ));
                }
            }
            lines.push(format!(
                "Policies: dependents={}, linked_issue={}",
                value["options"]["dependents"].as_str().unwrap_or_default(),
                value["options"]["linked_issue"]
                    .as_str()
                    .unwrap_or_default()
            ));
            if let Some(files) = value["effects"]["updated"].as_array() {
                for path in files {
                    lines.push(format!("  update {}", path.as_str().unwrap_or_default()));
                }
            }
            if let Some(actions) = value["effects"]["external"].as_array() {
                for action in actions {
                    lines.push(format!(
                        "  cancel {} via {}",
                        action["issue"].as_str().unwrap_or_default(),
                        action["adapter"].as_str().unwrap_or_default()
                    ));
                }
            }
            if let Ok(projection) = serde_json::from_value::<
                crate::tasks::removal::presentation::Projection,
            >(value["effects"]["relationships"].clone())
            {
                lines.extend(projection.lines());
            }
            append_problems(&mut lines, &value["blockers"], theme);
        }
        Some("result") => {
            lines.push(theme.success(&format!(
                "{}: {}",
                value["status"].as_str().unwrap_or_default(),
                value["operation_id"].as_str().unwrap_or_default()
            )));
            if let Some(targets) = value["removed"].as_array() {
                for target in targets {
                    lines.push(format!(
                        "  removed {}  {}",
                        target["task_id"].as_str().unwrap_or_default(),
                        target["task_name"].as_str().unwrap_or_default()
                    ));
                }
            }
        }
        Some("operation") => lines.push(theme.info(&format!(
            "Operation {}: {}",
            value["operation_id"].as_str().unwrap_or_default(),
            value["phase"].as_str().unwrap_or_default()
        ))),
        _ => lines.push(theme.error_line(
            "🔴",
            &format!(
                "{}: {}",
                value["code"].as_str().unwrap_or_default(),
                value["message"].as_str().unwrap_or_default()
            ),
        )),
    }
    if value["kind"] == "error" {
        for field in ["operation_id", "action_id", "stage", "cause"] {
            if let Some(detail) = value["details"][field].as_str() {
                lines.push(format!("  {field}: {detail}"));
            }
        }
    }
    append_problems(&mut lines, &value["warnings"], theme);
    lines.join("\n") + "\n"
}
fn append_problems(lines: &mut Vec<String>, problems: &Value, theme: Theme) {
    if let Some(problems) = problems.as_array() {
        for problem in problems {
            lines.push(theme.warning(&format!(
                "{}: {}",
                problem["code"].as_str().unwrap_or_default(),
                problem["message"].as_str().unwrap_or_default()
            )));
        }
    }
}
/// Only enumerated validation codes receive exit 2. Unknown failures stay operational.
pub(super) fn exit_code(problem: &Problem) -> i32 {
    match problem.code.as_str() {
        "invalid_removal_mode"
        | "removal_filters_not_supported"
        | "confirmation_required"
        | "stale_plan"
        | "mixed_target_kinds"
        | "habit_flag_on_task"
        | "habit_required"
        | "managed_task_cannot_delete"
        | "ambiguous_target"
        | "target_not_found"
        | "invalid_task_uuid"
        | "dependents_exist"
        | "missing_prerequisite"
        | "dependency_cycle"
        | "adapter_unavailable"
        | "shared_external_link"
        | "already_committed"
        | "actor_mismatch"
        | "operation_abandoned"
        | "invalid_removal_config"
        | "invalid_task_id"
        | "invalid_task_csv"
        | "invalid_project_metadata"
        | "invalid_workspace_path"
        | "duplicate_task_id"
        | "duplicate_task_uuid"
        | "invalid_project_path"
        | "dangling_reference"
        | "project_not_found"
        | "ambiguous_chunk_family"
        | "missing_last_touched_column"
        | "invalid_removal_integration" => 2,
        _ => 1,
    }
}
