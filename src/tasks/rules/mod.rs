//! `brain tasks lint` — the task automation rules.
//!
//! The canonical rule set (see the `/todo` skill's sync-rules reference) run as
//! code rather than as instructions. Dry run and `--fix` are the same pass with
//! one branch, so what the check reports is exactly what the fix would do.

pub(crate) mod links;
pub(crate) mod row;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use chrono::NaiveDate;

use crate::tasks::complete::{field, read_csv, write_csv};
use links::ProjectLinks;
use row::{Findings, Issue};

/// Everything one lint pass found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LintReport {
    pub(crate) fixes: Vec<String>,
    pub(crate) issues: Vec<String>,
}

impl LintReport {
    /// Nothing to fix and nothing to flag.
    pub(crate) fn is_clean(&self) -> bool {
        self.fixes.is_empty() && self.issues.is_empty()
    }
}

/// Read every project's `.METADATA.json` task list.
fn project_links(root: &Path) -> Vec<ProjectLinks> {
    let projects = root.join("projects");
    let Ok(entries) = std::fs::read_dir(&projects) else {
        return Vec::new();
    };
    let mut links: Vec<ProjectLinks> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = entry.path().join(".METADATA.json");
            let text = std::fs::read_to_string(&metadata).ok()?;
            let value: serde_json::Value = serde_json::from_str(&text).ok()?;
            let slug = value
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map_or_else(
                    || entry.file_name().to_string_lossy().into_owned(),
                    str::to_owned,
                );
            let listed = value
                .get("tasks")
                .and_then(serde_json::Value::as_array)
                .map(|ids| {
                    ids.iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            Some(ProjectLinks { slug, listed })
        })
        .collect();
    links.sort_by(|left, right| left.slug.cmp(&right.slug));
    links
}

/// Write a project's repaired task list back to its `.METADATA.json`.
fn save_project_tasks(root: &Path, slug: &str, tasks: &BTreeSet<String>) -> Result<()> {
    let path = root.join("projects").join(slug).join(".METADATA.json");
    let text = std::fs::read_to_string(&path)?;
    let mut value: serde_json::Value = serde_json::from_str(&text)?;
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "tasks".to_owned(),
            serde_json::Value::Array(
                tasks
                    .iter()
                    .map(|id| serde_json::Value::String(id.clone()))
                    .collect(),
            ),
        );
    }
    std::fs::write(&path, serde_json::to_string_pretty(&value)? + "\n")?;
    Ok(())
}

/// Run every rule over the workspace at `root`.
pub(crate) fn run(root: &Path, today: NaiveDate, fix: bool) -> Result<LintReport> {
    let tasks_dir = root.join("tasks");
    let mut report = LintReport::default();
    let mut forward: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for (file, is_tasks) in [("tasks.csv", true), ("habits.csv", false)] {
        let path = tasks_dir.join(file);
        if !path.exists() {
            continue;
        }
        let mut csv = read_csv(&path)?;
        let Findings {
            fixes,
            issues,
            changed,
        } = row::apply(&mut csv, file, is_tasks, today, fix);
        report.fixes.extend(fixes.into_iter().map(|fix| fix.0));
        report
            .issues
            .extend(issues.into_iter().map(|issue| issue.0));
        for csv_row in &csv.rows {
            let task_id = field(csv_row, "task_id");
            let slug = field(csv_row, "project").trim().to_owned();
            if !task_id.is_empty() && !slug.is_empty() {
                forward.entry(slug).or_default().insert(task_id);
            }
        }
        if fix && changed {
            write_csv(&path, &csv)?;
        }
    }

    let projects = project_links(root);
    let known: BTreeSet<String> = projects
        .iter()
        .map(|project| project.slug.clone())
        .collect();
    let found = links::reconcile(&forward, &known, &projects);
    report
        .issues
        .extend(found.issues.into_iter().map(|Issue(message)| message));
    for (slug, tasks) in found.repairs {
        if fix {
            save_project_tasks(root, &slug, &tasks)?;
            report.fixes.push(format!(
                "projects/{slug}: repaired the task list in .METADATA.json"
            ));
        } else {
            report.issues.push(format!(
                "link mismatch: task(s) point to project '{slug}' but .METADATA.json doesn't list them"
            ));
        }
    }
    Ok(report)
}

/// Pure: the themed report.
pub(crate) fn render(report: &LintReport, fix: bool, theme: crate::theme::Theme) -> String {
    use std::fmt::Write as _;

    if report.is_clean() {
        return format!("{}\n", theme.success("Task rules: all clean."));
    }
    let mut out = String::new();
    if !report.fixes.is_empty() {
        let _ = writeln!(
            out,
            "{}",
            theme.success(&format!("Applied {} fix(es):", report.fixes.len()))
        );
        for line in &report.fixes {
            let _ = writeln!(out, "  {} {}", theme.success("+"), theme.value(line));
        }
    }
    if !report.issues.is_empty() {
        let _ = writeln!(
            out,
            "{}",
            theme.warning(&format!("{} issue(s):", report.issues.len()))
        );
        for line in &report.issues {
            let _ = writeln!(out, "  {} {}", theme.warning("!"), theme.value(line));
        }
        if !fix {
            let _ = writeln!(
                out,
                "  {}",
                theme.muted("run with --fix to apply what can be applied")
            );
        }
    }
    out
}
