//! Per-row automation rules over `tasks.csv` and `habits.csv`.
//!
//! Every rule is pure: it looks at one row and reports a fix it *would* make or
//! an issue it can only flag. Nothing here writes, so `--fix` and the dry run
//! are the same code with one branch, and can never disagree about what is
//! wrong.

use chrono::NaiveDate;

use crate::tasks::complete::{CsvFile, Row, field, touch_row};

/// A correction the rules can make themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fix(pub(crate) String);

/// Something only a human can decide about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Issue(pub(crate) String);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Findings {
    pub(crate) fixes: Vec<Fix>,
    pub(crate) issues: Vec<Issue>,
    /// True when `apply` changed the CSV in memory.
    pub(crate) changed: bool,
}

impl Findings {
    fn fix(&mut self, message: String) {
        self.fixes.push(Fix(message));
    }

    fn issue(&mut self, message: String) {
        self.issues.push(Issue(message));
    }
}

fn label(row: &Row) -> String {
    format!("{} {}", field(row, "task_id"), field(row, "task_name"))
}

/// Does `notes` carry Markdown sub-task checkboxes?
///
/// A task with a checklist inside it is a project wearing a task's clothes;
/// the rules only flag it, because turning one into a project is a judgement.
pub(crate) fn has_checkboxes(notes: &str) -> bool {
    notes.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed
            .strip_prefix('-')
            .map(str::trim_start)
            .is_some_and(|rest| rest.starts_with("[ ]") || rest.starts_with("[x]"))
    })
}

/// Apply (or, with `fix` false, only report) every per-row rule.
pub(crate) fn apply(
    csv: &mut CsvFile,
    name: &str,
    is_tasks: bool,
    today: NaiveDate,
    fix: bool,
) -> Findings {
    let mut findings = Findings::default();
    let today_string = today.to_string();
    let has_last_touched = csv.header.iter().any(|column| column == "last_touched");
    if !has_last_touched {
        if fix {
            csv.header.push("last_touched".to_owned());
            findings.fix(format!("{name}: added last_touched column"));
            findings.changed = true;
        } else {
            findings.issue(format!(
                "{name}: missing last_touched column (--fix will add it and backfill {} row(s) from created_date)",
                csv.rows.len()
            ));
        }
    }
    let has_defer_count = csv.header.iter().any(|column| column == "defer_count");

    for row in &mut csv.rows {
        // Rule 1: a done row must record when it was done.
        if field(row, "status") == "done" && field(row, "completed_date").trim().is_empty() {
            if fix {
                row.insert("completed_date".to_owned(), today_string.clone());
                touch_row(row, &today_string);
                findings.fix(format!("{name}: set completed_date on '{}'", label(row)));
                findings.changed = true;
            } else {
                findings.issue(format!(
                    "{name}: '{}' is done but completed_date is empty",
                    label(row)
                ));
            }
        }
        // Rule 4: defer_count is an integer, defaulting to zero.
        if has_defer_count && field(row, "defer_count").trim().is_empty() {
            if fix {
                row.insert("defer_count".to_owned(), "0".to_owned());
                touch_row(row, &today_string);
                findings.changed = true;
            } else {
                findings.issue(format!("{name}: '{}' has empty defer_count", label(row)));
            }
        }
        // Rule 5: a habit in the tasks table is in the wrong file.
        if is_tasks && field(row, "task_type").contains("habit") {
            findings.issue(format!(
                "tasks.csv: '{}' has task_type=habit — should move to habits.csv",
                label(row)
            ));
        }
        // Rule 7: a checklist in the notes wants to be a project.
        if has_checkboxes(&field(row, "notes")) {
            findings.issue(format!(
                "{name}: '{}' has sub-task checkboxes in notes — consider turning it into a project",
                label(row)
            ));
        }
    }

    // Rule 8: every row carries a last_touched, backfilled from created_date.
    if csv.header.iter().any(|column| column == "last_touched") {
        let missing = csv
            .rows
            .iter()
            .filter(|row| field(row, "last_touched").trim().is_empty())
            .count();
        if missing > 0 {
            if fix {
                for row in &mut csv.rows {
                    if field(row, "last_touched").trim().is_empty() {
                        let created = field(row, "created_date");
                        let created = created.trim();
                        let value = if created.is_empty() {
                            today_string.clone()
                        } else {
                            created.to_owned()
                        };
                        row.insert("last_touched".to_owned(), value);
                    }
                }
                findings.fix(format!(
                    "{name}: backfilled last_touched on {missing} row(s) (from created_date)"
                ));
                findings.changed = true;
            } else {
                findings.issue(format!(
                    "{name}: {missing} row(s) have empty last_touched (run with --fix to backfill from created_date)"
                ));
            }
        }
    }

    findings
}
