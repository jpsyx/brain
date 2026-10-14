//! Resolve explicit needles against one immutable CSV snapshot.

use serde_json::{Value, json};

use crate::tasks::complete::{CsvFile, Located, Row, field, locate, normalize_id};
use crate::tasks::identity::{CsvKind, TaskUuid};
use crate::tasks::triage_habits::{ManagedTaskError, protect_system_key};

use super::model::{Options, Problem};

pub(crate) fn resolve_for_removal(
    tasks: &CsvFile,
    habits: &CsvFile,
    needles: &[String],
    options: &Options,
    managed_habits_enabled: bool,
) -> Result<Vec<(CsvKind, usize)>, Problem> {
    let selected = resolve_targets(tasks, habits, needles)?;
    if selected.iter().any(|(kind, _)| *kind == CsvKind::Tasks)
        && selected.iter().any(|(kind, _)| *kind == CsvKind::Habits)
    {
        return Err(Problem::new(
            "mixed_target_kinds",
            "a removal batch cannot contain both tasks and habits",
            json!({"targets": labels(tasks, habits, &selected)}),
        ));
    }
    for &(kind, index) in &selected {
        let row = selected_row(tasks, habits, kind, index);
        let details = label(row);
        match kind {
            CsvKind::Tasks if options.habit => {
                return Err(Problem::new(
                    "habit_flag_on_task",
                    "--habit cannot be used to remove a task",
                    details,
                ));
            }
            CsvKind::Habits => {
                protect_system_key(
                    &field(row, "system_key"),
                    managed_habits_enabled,
                    ManagedTaskError::ManagedTaskCannotDelete,
                )
                .map_err(|error| {
                    Problem::new(
                        "managed_task_cannot_delete",
                        &error.to_string(),
                        details.clone(),
                    )
                })?;
                if !options.habit {
                    return Err(Problem::new(
                        "habit_required",
                        "removing a habit requires --habit",
                        details,
                    ));
                }
            }
            CsvKind::Tasks => {}
        }
    }
    Ok(selected)
}

pub(crate) fn resolve_targets(
    tasks: &CsvFile,
    habits: &CsvFile,
    needles: &[String],
) -> Result<Vec<(CsvKind, usize)>, Problem> {
    let located: Vec<_> = needles
        .iter()
        .map(|needle| locate(tasks, habits, needle))
        .collect();
    let mut selected = Vec::with_capacity(needles.len());
    for (needle, result) in needles.iter().zip(located) {
        let target = match result.map_err(|error| {
            let candidates = matching_candidates(tasks, habits, needle);
            Problem::new(
                if candidates.len() > 1 {
                    "ambiguous_target"
                } else {
                    "target_not_found"
                },
                &error.to_string(),
                json!({"needle": needle, "candidates": labels(tasks, habits, &candidates)}),
            )
        })? {
            Located::Task(index) => (CsvKind::Tasks, index),
            Located::Habit(index) => (CsvKind::Habits, index),
        };
        let row = selected_row(tasks, habits, target.0, target.1);
        if let Some(value) = row
            .get("task_uuid")
            .filter(|value| !value.trim().is_empty())
        {
            TaskUuid::parse(value.trim()).map_err(|_| Problem::new(
                "invalid_task_uuid", "selected row has an invalid task UUID",
                json!({"needle": needle, "task_id": field(row, "task_id"), "task_name": field(row, "task_name"), "task_uuid": value}),
            ))?;
        }
        selected.push(target);
    }
    sort_targets(tasks, habits, &mut selected);
    selected.dedup();
    Ok(selected)
}

fn selected_row<'a>(
    tasks: &'a CsvFile,
    habits: &'a CsvFile,
    kind: CsvKind,
    index: usize,
) -> &'a Row {
    match kind {
        CsvKind::Tasks => &tasks.rows[index],
        CsvKind::Habits => &habits.rows[index],
    }
}

fn sort_targets(tasks: &CsvFile, habits: &CsvFile, targets: &mut [(CsvKind, usize)]) {
    targets.sort_by_key(|&(kind, index)| {
        let id = field(selected_row(tasks, habits, kind, index), "task_id");
        let number = normalize_id(&id)
            .ok()
            .and_then(|normalized| normalized[1..].parse::<u32>().ok())
            .unwrap_or(u32::MAX);
        (kind.as_str(), number, id, index)
    });
}

fn label(row: &Row) -> Value {
    json!({"task_id": field(row, "task_id"), "task_name": field(row, "task_name")})
}

fn labels(tasks: &CsvFile, habits: &CsvFile, targets: &[(CsvKind, usize)]) -> Vec<Value> {
    targets
        .iter()
        .map(|&(kind, index)| label(selected_row(tasks, habits, kind, index)))
        .collect()
}

fn matching_candidates(tasks: &CsvFile, habits: &CsvFile, needle: &str) -> Vec<(CsvKind, usize)> {
    let empty = CsvFile {
        header: vec![],
        rows: vec![],
    };
    let mut candidates = Vec::new();
    for (kind, csv) in [(CsvKind::Tasks, tasks), (CsvKind::Habits, habits)] {
        for (index, row) in csv.rows.iter().enumerate() {
            // Reuse the locator itself so diagnostics share every matching rule.
            let single = CsvFile {
                header: vec![],
                rows: vec![row.clone()],
            };
            let result = match kind {
                CsvKind::Tasks => locate(&single, &empty, needle),
                CsvKind::Habits => locate(&empty, &single, needle),
            };
            if result.is_ok() {
                candidates.push((kind, index));
            }
        }
    }
    sort_targets(tasks, habits, &mut candidates);
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_needles_resolve_to_one_identity() {
        let tasks = crate::tasks::complete::CsvFile {
            header: vec!["task_id".into(), "task_uuid".into(), "task_name".into()],
            rows: vec![std::collections::BTreeMap::from([
                ("task_id".into(), "T17".into()),
                (
                    "task_uuid".into(),
                    "11111111-1111-4111-8111-111111111111".into(),
                ),
                ("task_name".into(), "Send proposal".into()),
            ])],
        };
        let habits = crate::tasks::complete::CsvFile {
            header: vec![],
            rows: vec![],
        };
        let selected = resolve_targets(&tasks, &habits, &["T17".into(), "t17".into()]);
        assert_eq!(selected.expect("targets").len(), 1);
    }
}
