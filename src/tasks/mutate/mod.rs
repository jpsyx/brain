//! Native task/habit mutators: the operations `/todo` and `/triage` used to
//! reach through bundled Python scripts.
//!
//! Each one is the same shape as `complete`: a pure decision over CSV rows, a
//! thin root-scoped shell that reads, mutates, writes, and syncs the day's
//! agenda, and a `run` that resolves the workspace's targets. Reporting is a
//! pure formatter so the CLI's output is a checked contract.

pub(crate) mod assign;
pub(crate) mod backlog;
pub(crate) mod chunks;
pub(crate) mod defer;
pub(crate) mod remove;
pub(crate) mod touch;

#[cfg(test)]
mod tests;

use std::path::Path;

use anyhow::{Result, anyhow};

use crate::tasks::complete::{CsvFile, Located, Row, locate, read_csv};

/// One located row, with everything a mutator needs to write it back.
pub(crate) struct Target {
    pub(crate) csv: CsvFile,
    pub(crate) path: std::path::PathBuf,
    pub(crate) index: usize,
    pub(crate) is_habit: bool,
}

impl Target {
    pub(crate) fn row(&self) -> Result<&Row> {
        self.csv
            .rows
            .get(self.index)
            .ok_or_else(|| anyhow!("task row disappeared"))
    }

    pub(crate) fn row_mut(&mut self) -> Result<&mut Row> {
        self.csv
            .rows
            .get_mut(self.index)
            .ok_or_else(|| anyhow!("task row disappeared"))
    }

    pub(crate) fn ensure_column(&mut self, column: &str) {
        if !self.csv.header.iter().any(|existing| existing == column) {
            self.csv.header.push(column.to_owned());
        }
    }

    pub(crate) fn has_column(&self, column: &str) -> bool {
        self.csv.header.iter().any(|existing| existing == column)
    }
}

/// Resolve `raw_id` against both CSVs and load the one that holds it.
///
/// Deliberately searches both: a needle meant for a task can land on a habit,
/// and each mutator decides for itself whether that is allowed.
pub(crate) fn locate_target(root: &Path, raw_id: &str) -> Result<Target> {
    let tasks_dir = root.join("tasks");
    let tasks_path = tasks_dir.join("tasks.csv");
    let habits_path = tasks_dir.join("habits.csv");
    let tasks = read_csv(&tasks_path)?;
    let habits = read_csv(&habits_path)?;
    Ok(match locate(&tasks, &habits, raw_id)? {
        Located::Task(index) => Target {
            csv: tasks,
            path: tasks_path,
            index,
            is_habit: false,
        },
        Located::Habit(index) => Target {
            csv: habits,
            path: habits_path,
            index,
            is_habit: true,
        },
    })
}
