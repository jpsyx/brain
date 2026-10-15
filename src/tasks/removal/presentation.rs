//! Safe consent projection from the plan's captured inputs, without filesystem reads.
use super::model::RemovalPlan;
use crate::tasks::complete::{Row, field, normalize_id, parse_csv_bytes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Task {
    pub task_id: String,
    pub task_name: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Relationship {
    pub task: Task,
    pub field: String,
    pub previous: Vec<Task>,
    pub next: Vec<Task>,
}
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Project {
    pub path: String,
    pub previous: Vec<Task>,
    pub next: Vec<Task>,
}
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Handoff {
    pub removed: Task,
    pub successor: Task,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Projection {
    pub tasks: Vec<Relationship>,
    pub projects: Vec<Project>,
    pub handoffs: Vec<Handoff>,
}

pub(crate) fn project(plan: &RemovalPlan) -> Projection {
    if !plan.blockers.is_empty() {
        return Projection::default();
    }
    let mut names = plan.presentation_labels.clone();
    for edit in &plan.edits {
        if is_task_csv(&edit.relative) {
            for row in rows(edit.before.as_deref().unwrap_or_default()) {
                let id = field(&row, "task_id");
                if let Ok(key) = normalize_id(&id) {
                    names.insert(
                        key,
                        Task {
                            task_id: id,
                            task_name: field(&row, "task_name"),
                        },
                    );
                }
            }
        }
    }
    for target in &plan.targets {
        if let Ok(key) = normalize_id(&target.task_id) {
            names.insert(
                key,
                Task {
                    task_id: target.task_id.clone(),
                    task_name: target.task_name.clone(),
                },
            );
        }
    }
    let mut output = Projection::default();
    for edit in &plan.edits {
        if is_task_csv(&edit.relative) {
            let before: BTreeMap<_, _> = rows(edit.before.as_deref().unwrap_or_default())
                .into_iter()
                .map(|row| (field(&row, "task_id"), row))
                .collect();
            for row in rows(&edit.after) {
                let id = field(&row, "task_id");
                let Some(original) = before.get(&id) else {
                    continue;
                };
                for (column, label) in [("blocked_by", "dependencies"), ("see_also", "references")]
                {
                    let previous = edges(&field(original, column), &names);
                    let next = edges(&field(&row, column), &names);
                    if previous != next {
                        if let Some(task) = named(&id, &names) {
                            output.tasks.push(Relationship {
                                task,
                                field: label.into(),
                                previous,
                                next,
                            });
                        }
                    }
                }
            }
        } else if edit.relative.ends_with(".METADATA.json") {
            let previous = project_tasks(edit.before.as_deref().unwrap_or_default(), &names);
            let next = project_tasks(&edit.after, &names);
            if previous != next {
                output.projects.push(Project {
                    path: edit.relative.to_string_lossy().into_owned(),
                    previous,
                    next,
                });
            }
        }
    }
    for (removed, successor) in &plan.successors {
        if let (Some(removed), Some(successor)) = (named(removed, &names), named(successor, &names))
        {
            output.handoffs.push(Handoff { removed, successor });
        }
    }
    output
}
fn is_task_csv(path: &std::path::Path) -> bool {
    path == std::path::Path::new("tasks/tasks.csv")
        || path == std::path::Path::new("tasks/habits.csv")
}
fn rows(bytes: &[u8]) -> Vec<Row> {
    parse_csv_bytes(bytes).map_or_else(|_| Vec::new(), |csv| csv.rows)
}
fn named(id: &str, names: &BTreeMap<String, Task>) -> Option<Task> {
    let key = normalize_id(id).ok()?;
    Some(names.get(&key).cloned().unwrap_or_else(|| Task {
        task_id: id.trim().into(),
        task_name: String::new(),
    }))
}
fn edges(value: &str, names: &BTreeMap<String, Task>) -> Vec<Task> {
    value.split('|').filter_map(|id| named(id, names)).collect()
}
fn project_tasks(bytes: &[u8], names: &BTreeMap<String, Task>) -> Vec<Task> {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|value| {
            value
                .get("tasks")
                .and_then(serde_json::Value::as_array)
                .cloned()
        })
        .unwrap_or_default()
        .iter()
        .filter_map(|id| id.as_str().and_then(|id| named(id, names)))
        .collect()
}
fn label(task: &Task) -> String {
    if task.task_name.is_empty() {
        task.task_id.clone()
    } else {
        format!("{} ({})", task.task_id, task.task_name)
    }
}
fn labels(tasks: &[Task]) -> String {
    if tasks.is_empty() {
        "none".into()
    } else {
        tasks.iter().map(label).collect::<Vec<_>>().join("|")
    }
}
impl Projection {
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut lines: Vec<_> = self
            .tasks
            .iter()
            .map(|change| {
                format!(
                    "{} {}: {} -> {}",
                    if change.field == "dependencies" {
                        "Dependencies"
                    } else {
                        "References"
                    },
                    label(&change.task),
                    labels(&change.previous),
                    labels(&change.next)
                )
            })
            .collect();
        lines.extend(self.projects.iter().map(|change| {
            format!(
                "Project tasks {}: {} -> {}",
                change.path,
                labels(&change.previous),
                labels(&change.next)
            )
        }));
        lines.extend(self.handoffs.iter().map(|change| {
            format!(
                "Agenda handoff {} to {}",
                label(&change.removed),
                label(&change.successor)
            )
        }));
        lines
    }
}

/// Retain only names used by affected edges, project memberships or handoffs.
pub(super) fn capture_labels(snapshot: &super::snapshot::Snapshot, plan: &mut RemovalPlan) {
    let projection = project(plan);
    let mut required = std::collections::BTreeSet::new();
    let mut remember = |task: &Task| {
        if let Ok(id) = normalize_id(&task.task_id) {
            required.insert(id);
        }
    };
    for change in &projection.tasks {
        remember(&change.task);
        for task in change.previous.iter().chain(&change.next) {
            remember(task);
        }
    }
    for change in &projection.projects {
        for task in change.previous.iter().chain(&change.next) {
            remember(task);
        }
    }
    for change in &projection.handoffs {
        remember(&change.removed);
        remember(&change.successor);
    }
    for row in snapshot
        .tasks
        .parsed
        .rows
        .iter()
        .chain(&snapshot.habits.parsed.rows)
    {
        let id = field(row, "task_id");
        if let Ok(key) = normalize_id(&id) {
            if required.contains(&key) {
                plan.presentation_labels.insert(
                    key,
                    Task {
                        task_id: id,
                        task_name: field(row, "task_name"),
                    },
                );
            }
        }
    }
}

pub(super) fn enrich_dependency(
    mut problem: super::model::Problem,
    originals: &[&Row],
) -> super::model::Problem {
    if problem.code == "dependents_exist" {
        for (id_field, name_field) in [
            ("task_id", "task_name"),
            ("prerequisite", "prerequisite_name"),
        ] {
            if let Some(id) = problem.details[id_field].as_str() {
                if let Some(row) = originals
                    .iter()
                    .find(|row| super::plan::key(&field(row, "task_id")) == id)
                {
                    problem.details[name_field] = serde_json::json!(field(row, "task_name"));
                }
            }
        }
        problem.message = format!(
            "{} ({}) depends on removed {} ({})",
            problem.details["task_id"].as_str().unwrap_or_default(),
            problem.details["task_name"].as_str().unwrap_or_default(),
            problem.details["prerequisite"].as_str().unwrap_or_default(),
            problem.details["prerequisite_name"]
                .as_str()
                .unwrap_or_default()
        );
    }
    problem
}
