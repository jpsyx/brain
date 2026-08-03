//! `brain habits skip <id|fuzzy>` — opt out of a habit for today, with
//! semantics that depend on the habit's cadence.
//!
//! Native port of the old `skip_habit.py`. The decision is deterministic:
//!
//! - **Daily habit** (`recur_interval == 1` AND `recur_unit == days`) → today's
//!   occurrence is "handled": mark it `done` (records `completed_date=today`)
//!   and spawn tomorrow's occurrence, exactly like completion. A daily habit is
//!   back tomorrow regardless, so "skip today" *is* "today is handled".
//! - **Non-daily habit** (weekly, monthly, every-N-days, …) → do **not** mark
//!   it done; defer its `due_date` to tomorrow (today + 1 day). Skipping a
//!   non-daily habit is a one-day defer — the instance simply reappears tomorrow.
//! - **`--until YYYY-MM-DD`** (either cadence) → defer the `due_date` to that
//!   day, never marking it done. Must be strictly after today.
//!
//! This is the "not today" lever; deferring a *whole* recurrence interval is a
//! separate concern (`/todo defer-habit`). Skip only operates on habits — a
//! task id is rejected with a pointer to `brain tasks complete`.

use std::path::Path;

use anyhow::{Result, anyhow, bail};
use chrono::{Days, Local, NaiveDate};

use super::complete::{Located, Row, field, locate, read_csv, spawn_next_occurrence, write_csv};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipKind {
    /// Daily habit: today's occurrence was completed and the next spawned.
    MarkedDone,
    /// Non-daily / `--until`: the `due_date` was moved forward, not completed.
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipResult {
    pub kind: SkipKind,
    pub task_id: String,
    pub task_name: String,
    /// `Deferred` only: the previous `due_date`.
    pub old_due: Option<String>,
    /// `Deferred` only: the new `due_date`.
    pub new_due: Option<String>,
    /// `MarkedDone` only: the spawned occurrence's id.
    pub next_id: Option<String>,
    /// `MarkedDone` only: the spawned occurrence's `due_date`.
    pub next_due: Option<String>,
}

/// CLI runner for `brain habits skip <id|fuzzy> [--until YYYY-MM-DD]`.
pub fn run(root: &Path, raw_id: &str, until: Option<&str>) -> Result<()> {
    crate::logging::log(format!("habits skip raw_id={raw_id} until={until:?}"));
    let today = Local::now().date_naive();
    let until = until.map(parse_until).transpose()?;
    let result = skip_in_root_with_today(root, raw_id, until, today)?;
    print_result(&result);
    Ok(())
}

fn parse_until(raw: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| anyhow!("--until must be YYYY-MM-DD, got '{raw}'"))
}

pub fn skip_in_root_with_today(
    root: &Path,
    raw_id: &str,
    until: Option<NaiveDate>,
    today: NaiveDate,
) -> Result<SkipResult> {
    let tasks_dir = root.join("tasks");
    let tasks = read_csv(&tasks_dir.join("tasks.csv"))?;
    let habits_path = tasks_dir.join("habits.csv");
    let mut habits = read_csv(&habits_path)?;

    let idx = match locate(&tasks, &habits, raw_id)? {
        Located::Habit(idx) => idx,
        Located::Task(_) => bail!(
            "skip only operates on habits; '{raw_id}' is a task. \
             Use `brain tasks complete {raw_id}` to finish a task."
        ),
    };

    // `--until` overrides cadence: defer to the target day, never mark done.
    if let Some(target) = until {
        if target <= today {
            bail!("--until must be strictly after today ({today}); got {target}");
        }
        return defer(&habits_path, &mut habits, idx, &target.to_string(), today);
    }

    // Daily habit → today's occurrence is handled: mark done + spawn the next.
    if is_daily(row_at(&habits, idx)?) {
        let today_s = today.to_string();
        {
            let row = habits
                .rows
                .get_mut(idx)
                .ok_or_else(|| anyhow!("habit row disappeared"))?;
            row.insert("status".to_owned(), "done".to_owned());
            row.insert("completed_date".to_owned(), today_s.clone());
            row.insert("last_touched".to_owned(), today_s);
        }
        let (task_id, task_name) = {
            let row = row_at(&habits, idx)?;
            (field(row, "task_id"), field(row, "task_name"))
        };
        let (next_id, next_due) = spawn_next_occurrence(&tasks_dir, &mut habits, idx, today)?;
        write_csv(&habits_path, &habits)?;
        return Ok(SkipResult {
            kind: SkipKind::MarkedDone,
            task_id,
            task_name,
            old_due: None,
            new_due: None,
            next_id: Some(next_id),
            next_due: Some(next_due),
        });
    }

    // Non-daily habit → one-day defer.
    let tomorrow = today
        .checked_add_days(Days::new(1))
        .ok_or_else(|| anyhow!("date overflow computing tomorrow"))?;
    defer(&habits_path, &mut habits, idx, &tomorrow.to_string(), today)
}

fn defer(
    habits_path: &Path,
    habits: &mut super::complete::CsvFile,
    idx: usize,
    new_due: &str,
    today: NaiveDate,
) -> Result<SkipResult> {
    let today_s = today.to_string();
    let (task_id, task_name, old_due) = {
        let row = habits
            .rows
            .get_mut(idx)
            .ok_or_else(|| anyhow!("habit row disappeared"))?;
        let old_due = field(row, "due_date");
        row.insert("due_date".to_owned(), new_due.to_owned());
        row.insert("last_touched".to_owned(), today_s);
        (field(row, "task_id"), field(row, "task_name"), old_due)
    };
    write_csv(habits_path, habits)?;
    Ok(SkipResult {
        kind: SkipKind::Deferred,
        task_id,
        task_name,
        old_due: Some(old_due),
        new_due: Some(new_due.to_owned()),
        next_id: None,
        next_due: None,
    })
}

fn row_at(habits: &super::complete::CsvFile, idx: usize) -> Result<&Row> {
    habits
        .rows
        .get(idx)
        .ok_or_else(|| anyhow!("habit row disappeared"))
}

fn is_daily(row: &Row) -> bool {
    let interval = field(row, "recur_interval")
        .trim()
        .parse::<u32>()
        .unwrap_or(0);
    let unit = field(row, "recur_unit").trim().to_ascii_lowercase();
    interval == 1 && unit == "days"
}

fn print_result(result: &SkipResult) {
    let theme = Theme::active();
    match result.kind {
        SkipKind::MarkedDone => {
            eprintln!(
                "{} {}  {}  {}",
                theme.success("skipped (daily → done):"),
                theme.accent(&result.task_id),
                theme.value(&result.task_name),
                theme.muted("(habit)")
            );
            if let (Some(id), Some(due)) = (&result.next_id, &result.next_due) {
                eprintln!(
                    "  {} {} {} {}",
                    theme.info("next occurrence:"),
                    theme.accent(id),
                    theme.muted("due"),
                    theme.value(due)
                );
            }
        }
        SkipKind::Deferred => {
            eprintln!(
                "{} {}  {}  {}",
                theme.success("skipped:"),
                theme.accent(&result.task_id),
                theme.value(&result.task_name),
                theme.muted("(habit)")
            );
            if let (Some(old), Some(new)) = (&result.old_due, &result.new_due) {
                eprintln!(
                    "  {} {} {} {}",
                    theme.muted("due_date:"),
                    theme.value(old),
                    theme.muted("→"),
                    theme.value(new)
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SkipKind, skip_in_root_with_today};
    use chrono::NaiveDate;

    const HABITS_HEADER: &str = "task_id,task_name,status,due_date,recur_interval,recur_unit,ideal_time,created_date,completed_date,last_touched";

    fn fixture(habits_rows: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("tasks.csv"),
            "task_id,task_name,status,completed_date,last_touched\n\
             T1,Some task,not_started,,\n",
        )
        .unwrap();
        std::fs::write(tasks_dir.join(".habits_next_id"), "900\n").unwrap();
        std::fs::write(
            tasks_dir.join("habits.csv"),
            format!("{HABITS_HEADER}\n{habits_rows}"),
        )
        .unwrap();
        dir
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 31).unwrap()
    }

    #[test]
    fn daily_skip_marks_done_and_spawns_next() {
        let dir = fixture("H35,Morning Triage,not_started,2026-07-31,1,days,09:00,2026-07-31,,\n");
        let result = skip_in_root_with_today(dir.path(), "H35", None, today()).unwrap();

        assert_eq!(result.kind, SkipKind::MarkedDone);
        assert_eq!(result.next_due.as_deref(), Some("2026-08-01"));

        let csv = std::fs::read_to_string(dir.path().join("tasks/habits.csv")).unwrap();
        // Today's occurrence is completed…
        assert!(
            csv.contains(
                "H35,Morning Triage,done,2026-07-31,1,days,09:00,2026-07-31,2026-07-31,2026-07-31"
            ),
            "completed row missing; got:\n{csv}"
        );
        // …and tomorrow's occurrence is spawned, not_started.
        assert!(
            csv.contains("Morning Triage,not_started,2026-08-01,1,days"),
            "spawned row missing; got:\n{csv}"
        );
    }

    #[test]
    fn non_daily_skip_defers_to_tomorrow_without_completing() {
        let dir = fixture("H40,Weekly review,not_started,2026-07-31,1,weeks,,2026-07-24,,\n");
        let result = skip_in_root_with_today(dir.path(), "H40", None, today()).unwrap();

        assert_eq!(result.kind, SkipKind::Deferred);
        assert_eq!(result.new_due.as_deref(), Some("2026-08-01"));

        let csv = std::fs::read_to_string(dir.path().join("tasks/habits.csv")).unwrap();
        // Deferred to tomorrow, still not_started, no new row spawned.
        assert!(
            csv.contains("H40,Weekly review,not_started,2026-08-01,1,weeks"),
            "deferred row wrong; got:\n{csv}"
        );
        assert_eq!(csv.matches("Weekly review").count(), 1);
    }

    #[test]
    fn skip_until_defers_to_that_day_for_a_daily_habit() {
        let dir = fixture("H35,Morning Triage,not_started,2026-07-31,1,days,09:00,2026-07-31,,\n");
        let result = skip_in_root_with_today(
            dir.path(),
            "H35",
            NaiveDate::from_ymd_opt(2026, 8, 10),
            today(),
        )
        .unwrap();

        assert_eq!(result.kind, SkipKind::Deferred);
        assert_eq!(result.new_due.as_deref(), Some("2026-08-10"));

        let csv = std::fs::read_to_string(dir.path().join("tasks/habits.csv")).unwrap();
        // Never marked done; just deferred. Only the one row.
        assert!(
            csv.contains("H35,Morning Triage,not_started,2026-08-10,1,days"),
            "until-deferred row wrong; got:\n{csv}"
        );
        assert_eq!(csv.matches("Morning Triage").count(), 1);
    }

    #[test]
    fn skip_until_must_be_strictly_after_today() {
        let dir = fixture("H35,Morning Triage,not_started,2026-07-31,1,days,09:00,2026-07-31,,\n");
        assert!(skip_in_root_with_today(dir.path(), "H35", Some(today()), today()).is_err());
    }

    #[test]
    fn skipping_a_task_is_rejected() {
        let dir = fixture("H35,Morning Triage,not_started,2026-07-31,1,days,,2026-07-31,,\n");
        let err = skip_in_root_with_today(dir.path(), "T1", None, today()).unwrap_err();
        assert!(err.to_string().contains("complete"));
    }
}
