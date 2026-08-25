//! `brain tasks defer <id> <when>` — push a task out, with the penalty.
//!
//! `defer_count` is the "are we avoiding this?" signal, so it only climbs when
//! the delay is actually ours. A task that is `waiting` on someone else, or
//! `blocked_by` another task, defers for free — and so does an explicit
//! `--no-count` for any other genuinely-not-our-fault push.
//!
//! The penalty, when it applies, is the **defer-demote rule**: a deferred task
//! sheds its `mit` tag, and a `p0` drops to `p1`. If it can wait, it is no
//! longer both urgent and critical.

use std::path::Path;

use anyhow::{Result, anyhow, bail};
use chrono::NaiveDate;

use super::chunks::{self, Cascaded};
use super::locate_target;
use crate::tasks::agenda::{Action, Outcome, Targets, sync_targets};
use crate::tasks::complete::{field, touch_row, write_csv};

/// Where to push the task to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum When {
    /// `+Nd`: N days past the row's own due date (or today, if it has none).
    Days(u32),
    /// An absolute `YYYY-MM-DD`.
    On(NaiveDate),
}

impl When {
    /// Parse the CLI spelling. Deliberately strict: a defer that guesses at
    /// "next tuesday" is a defer that lands on the wrong day.
    pub(crate) fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if let Some(days) = raw.strip_prefix('+').and_then(|rest| {
            rest.strip_suffix('d')
                .filter(|digits| !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
        }) {
            return days
                .parse()
                .map(Self::Days)
                .map_err(|error| anyhow!("'{raw}' is not a day count: {error}"));
        }
        NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .map(Self::On)
            .map_err(|_| anyhow!("unknown date format '{raw}' (expected +Nd or YYYY-MM-DD)"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeferResult {
    pub(crate) task_id: String,
    pub(crate) task_name: String,
    pub(crate) old_due: String,
    pub(crate) new_due: String,
    pub(crate) old_defer_count: u32,
    pub(crate) new_defer_count: u32,
    /// Why no penalty applied (`waiting`, `blocked`, `--no-count`), if none did.
    pub(crate) no_penalty_reason: Option<&'static str>,
    pub(crate) dropped_mit: bool,
    /// `(before, after)` when the defer-demote rule changed the priority.
    pub(crate) demoted_priority: Option<(String, String)>,
    pub(crate) cascaded: Vec<Cascaded>,
    /// An external tracker id this binary cannot reach; the caller must.
    pub(crate) linear_issue: Option<String>,
}

pub(crate) fn defer_in_root(
    root: &Path,
    targets: &Targets,
    raw_id: &str,
    when: When,
    force_no_count: bool,
    today: NaiveDate,
) -> Result<(DeferResult, Outcome)> {
    let mut target = locate_target(root, raw_id)?;
    if target.is_habit {
        let id = field(target.row()?, "task_id");
        bail!(
            "{id} is a habit; a habit's recurrence is its deferral mechanism. \
             Use `{}` to skip its next occurrence.",
            crate::workspace::suggest(&format!("habits defer {id}"))
        );
    }
    let has_defer_count = target.has_column("defer_count");
    let today_string = today.to_string();

    let row = target.row()?;
    let old_due = field(row, "due_date");
    let old_priority = field(row, "priority");
    let old_types = field(row, "task_type");
    let old_defer_count: u32 = field(row, "defer_count").trim().parse().unwrap_or(0);
    let waiting = field(row, "status").trim() == "waiting";
    let blocked = !field(row, "blocked_by").trim().is_empty();
    let linear_issue = Some(field(row, "linear_issue"))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    let no_penalty_reason = if force_no_count {
        Some("--no-count")
    } else if waiting {
        Some("waiting")
    } else if blocked {
        Some("blocked")
    } else {
        None
    };

    let new_due = match when {
        When::Days(days) => chunks::shift(&old_due, days, today).to_string(),
        When::On(date) => date.to_string(),
    };

    let row = target.row_mut()?;
    row.insert("due_date".to_owned(), new_due.clone());
    let mut new_defer_count = old_defer_count;
    let mut dropped_mit = false;
    let mut demoted_priority = None;
    if no_penalty_reason.is_none() {
        if has_defer_count {
            new_defer_count = old_defer_count.saturating_add(1);
            row.insert("defer_count".to_owned(), new_defer_count.to_string());
        }
        let kept: Vec<&str> = old_types
            .split('|')
            .filter(|part| !part.is_empty() && *part != "mit")
            .collect();
        if kept.len() != old_types.split('|').filter(|part| !part.is_empty()).count() {
            dropped_mit = true;
            row.insert("task_type".to_owned(), kept.join("|"));
        }
        if old_priority.trim() == "p0" {
            row.insert("priority".to_owned(), "p1".to_owned());
            demoted_priority = Some((old_priority, "p1".to_owned()));
        }
    }
    touch_row(row, &today_string);

    let cascaded = chunks::cascade_forward(&mut target.csv.rows, target.index, &today_string);
    target.ensure_column("last_touched");
    write_csv(&target.path, &target.csv)?;

    let task_id = field(target.row()?, "task_id");
    let task_name = field(target.row()?, "task_name");
    let mut outcome = sync_targets(targets, &task_id, Action::Defer, today);
    for chunk in &cascaded {
        // A cascaded chunk left today's plan too.
        let cascaded_outcome = sync_targets(targets, &chunk.task_id, Action::Defer, today);
        if matches!(cascaded_outcome, Outcome::Updated { .. }) {
            outcome = cascaded_outcome;
        }
    }

    Ok((
        DeferResult {
            task_id,
            task_name,
            old_due,
            new_due,
            old_defer_count,
            new_defer_count,
            no_penalty_reason,
            dropped_mit,
            demoted_priority,
            cascaded,
            linear_issue,
        },
        outcome,
    ))
}
