mod assign;
mod backlog;
mod defer;
mod remove;
mod touch;

use std::path::Path;

use chrono::NaiveDate;

pub(super) const TASKS_HEADER: &str = "task_id,task_name,task_type,status,priority,due_date,start_date,hard_deadline,assigned_to,blocked_by,defer_count,backlogged_date,waiting_since,project,linear_issue,created_date,completed_date,last_touched";
pub(super) const HABITS_HEADER: &str = "task_id,task_name,status,priority,due_date,recur_interval,recur_unit,assigned_to,created_date,completed_date,last_touched,system_key";

pub(super) fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 8, 24).expect("valid date")
}

/// A workspace whose CSVs hold `tasks` and `habits` rows (header-less bodies).
pub(super) struct Fixture {
    _dir: tempfile::TempDir,
    pub(super) root: std::path::PathBuf,
}

impl Fixture {
    pub(super) fn tasks(&self) -> String {
        std::fs::read_to_string(self.root.join("tasks/tasks.csv")).expect("read tasks")
    }

    pub(super) fn habits(&self) -> String {
        std::fs::read_to_string(self.root.join("tasks/habits.csv")).expect("read habits")
    }

    /// The agenda targets, pointed at a directory with no agenda in it, so the
    /// sync every mutator runs is a clean no-op these tests can ignore.
    pub(super) fn targets(&self) -> crate::tasks::agenda::Targets {
        crate::tasks::agenda::Targets {
            markdown: self.root.join("no-agenda/2026-08-24.md"),
            pdf: self.root.join("no-agenda/agenda.pdf"),
            renderer: None,
            tasks_dir: self.root.join("tasks"),
        }
    }
}

pub(super) fn fixture(tasks: &str, habits: &str) -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("brain");
    std::fs::create_dir_all(root.join("tasks")).expect("tasks dir");
    std::fs::write(
        root.join("tasks/tasks.csv"),
        format!("{TASKS_HEADER}\n{tasks}"),
    )
    .expect("tasks.csv");
    std::fs::write(
        root.join("tasks/habits.csv"),
        format!("{HABITS_HEADER}\n{habits}"),
    )
    .expect("habits.csv");
    Fixture { _dir: dir, root }
}

pub(super) fn column(csv: &str, task_id: &str, header: &str, column: &str) -> String {
    let index = header
        .split(',')
        .position(|name| name == column)
        .expect("known column");
    csv.lines()
        .skip(1)
        .find(|line| line.starts_with(&format!("{task_id},")))
        .unwrap_or_else(|| panic!("row {task_id} in\n{csv}"))
        .split(',')
        .nth(index)
        .unwrap_or_default()
        .to_owned()
}

pub(super) fn users_json(root: &Path, ids: &[&str]) {
    let users: Vec<String> = ids
        .iter()
        .map(|id| format!(r#"{{"id":"{id}","name":"{id}"}}"#))
        .collect();
    std::fs::create_dir_all(root.join(".config")).expect("config dir");
    std::fs::write(
        root.join(".config/users.json"),
        format!(r#"{{"schema_version":1,"users":[{}]}}"#, users.join(",")),
    )
    .expect("users.json");
}
