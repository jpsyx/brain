//! Human consequences from the shared captured-plan projection, without IO.
use crate::tasks::removal::{
    model::RemovalPlan,
    presentation::{self, Task},
};

pub(super) fn relationships(plan: &RemovalPlan) -> Vec<String> {
    let projection = presentation::project(plan);
    let mut lines: Vec<_> = projection
        .tasks
        .iter()
        .map(|change| {
            let task = label(&change.task);
            let previous = labels(&change.previous);
            let next = labels(&change.next);
            if change.field == "dependencies" {
                if change.next.is_empty() {
                    format!("{task} will no longer wait for other tasks (previously {previous}).")
                } else {
                    format!("{task} will wait for {next} (previously {previous}).")
                }
            } else {
                format!("{task} will link to {next} (previously {previous}).")
            }
        })
        .collect();
    lines.extend(projection.projects.iter().map(|change| {
        let path = std::path::Path::new(&change.path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new(&change.path));
        format!(
            "Project {} will list {} (previously {}).",
            path.display(),
            labels(&change.next),
            labels(&change.previous)
        )
    }));
    lines.extend(projection.handoffs.iter().map(|change| {
        format!(
            "{} takes the place of {} in the agenda.",
            label(&change.successor),
            label(&change.removed)
        )
    }));
    lines
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
        tasks.iter().map(label).collect::<Vec<_>>().join(", ")
    }
}
