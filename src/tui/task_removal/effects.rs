//! Presentation-only comparison of the plan's already captured before/after bytes.
use crate::tasks::removal::model::RemovalPlan;
use std::collections::BTreeMap;
pub(super) fn relationships(plan: &RemovalPlan) -> Vec<String> {
    let mut lines = Vec::new();
    for edit in &plan.edits {
        if edit
            .relative
            .extension()
            .is_some_and(|extension| extension == "csv")
        {
            let before = rows(edit.before.as_deref().unwrap_or_default());
            for (id, fields) in rows(&edit.after) {
                let Some(original) = before.get(&id) else {
                    continue;
                };
                for (index, label) in [(0, "Dependencies"), (1, "References")] {
                    if fields[index] != original[index] {
                        lines.push(format!(
                            "{label} {id}: {} -> {}",
                            shown(&original[index]),
                            shown(&fields[index])
                        ));
                    }
                }
            }
        } else if edit.relative.ends_with(".METADATA.json") {
            let tasks = |bytes: &[u8]| {
                serde_json::from_slice::<serde_json::Value>(bytes)
                    .ok()
                    .and_then(|value| value.get("tasks").cloned())
                    .unwrap_or(serde_json::json!([]))
            };
            let before = tasks(edit.before.as_deref().unwrap_or_default());
            let after = tasks(&edit.after);
            if before != after {
                lines.push(format!(
                    "Project tasks {}: {before} -> {after}",
                    edit.relative.display()
                ));
            }
        }
    }
    lines
}
fn shown(value: &str) -> &str {
    if value.is_empty() { "none" } else { value }
}
fn rows(bytes: &[u8]) -> BTreeMap<String, [String; 2]> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(bytes);
    let Ok(headers) = reader.headers() else {
        return BTreeMap::new();
    };
    let id = headers.iter().position(|header| header == "task_id");
    let columns =
        ["blocked_by", "see_also"].map(|field| headers.iter().position(|header| header == field));
    reader
        .records()
        .filter_map(Result::ok)
        .filter_map(|record| {
            Some((
                record.get(id?)?.into(),
                columns.map(|column| {
                    column
                        .and_then(|column| record.get(column))
                        .unwrap_or_default()
                        .into()
                }),
            ))
        })
        .collect()
}
