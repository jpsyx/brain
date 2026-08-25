//! Named day streaks.
//!
//! "How many days in a row did *X* happen?" is calendar arithmetic, and an LLM
//! counting it in-context gets it wrong and gets it wrong differently each
//! time. So the dates are persisted and the counting is code.
//!
//! Deliberately generic: core stores a **named** set of dates and counts the
//! run. What the name means — a late working night, a streak of morning runs,
//! anything else — is entirely the caller's, so no particular habit or workflow
//! is baked into the binary.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    dates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct StreakStatus {
    pub(crate) name: String,
    pub(crate) target_date: String,
    /// Consecutive days ending at (or the day before) `target_date`.
    pub(crate) streak: u32,
    pub(crate) last_marked: Option<String>,
    pub(crate) total_marked: usize,
}

/// A streak name is a file name, so it is kept to the safe subset.
pub(crate) fn validate_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    let ok = !trimmed.is_empty()
        && trimmed.len() <= 64
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(trimmed.to_owned())
    } else {
        bail!("'{name}' is not a streak name (letters, digits, '-' and '_', up to 64 characters)")
    }
}

pub(crate) fn state_path(root: &Path, name: &str) -> PathBuf {
    root.join("tasks/.streaks").join(format!("{name}.json"))
}

fn load(root: &Path, name: &str) -> BTreeSet<NaiveDate> {
    std::fs::read_to_string(state_path(root, name))
        .ok()
        .and_then(|text| serde_json::from_str::<Stored>(&text).ok())
        .map(|stored| {
            stored
                .dates
                .iter()
                .filter_map(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
                .collect()
        })
        .unwrap_or_default()
}

fn save(root: &Path, name: &str, dates: &BTreeSet<NaiveDate>) -> Result<()> {
    let path = state_path(root, name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stored = Stored {
        dates: dates.iter().map(ToString::to_string).collect(),
    };
    std::fs::write(&path, serde_json::to_string_pretty(&stored)? + "\n")?;
    Ok(())
}

/// Consecutive marked days ending at `target`.
///
/// If `target` itself is unmarked but the day before is, the count anchors
/// there: the user has not decided about today yet, and yesterday's run is
/// still real. Any older gap means the streak is broken, and broken is zero.
pub(crate) fn run_ending(target: NaiveDate, dates: &BTreeSet<NaiveDate>) -> u32 {
    let mut cursor = if dates.contains(&target) {
        target
    } else {
        let yesterday = target - Days::new(1);
        if dates.contains(&yesterday) {
            yesterday
        } else {
            return 0;
        }
    };
    let mut count = 0;
    while dates.contains(&cursor) {
        count += 1;
        let Some(previous) = cursor.checked_sub_days(Days::new(1)) else {
            break;
        };
        cursor = previous;
    }
    count
}

fn status_of(name: &str, target: NaiveDate, dates: &BTreeSet<NaiveDate>) -> StreakStatus {
    StreakStatus {
        name: name.to_owned(),
        target_date: target.to_string(),
        streak: run_ending(target, dates),
        last_marked: dates.iter().next_back().map(ToString::to_string),
        total_marked: dates.len(),
    }
}

pub(crate) fn status(root: &Path, name: &str, target: NaiveDate) -> Result<StreakStatus> {
    let name = validate_name(name)?;
    Ok(status_of(&name, target, &load(root, &name)))
}

/// Record `target`. Idempotent.
pub(crate) fn mark(root: &Path, name: &str, target: NaiveDate) -> Result<StreakStatus> {
    let name = validate_name(name)?;
    let mut dates = load(root, &name);
    dates.insert(target);
    save(root, &name, &dates)?;
    Ok(status_of(&name, target, &dates))
}

/// Forget `target`. Idempotent.
pub(crate) fn unmark(root: &Path, name: &str, target: NaiveDate) -> Result<StreakStatus> {
    let name = validate_name(name)?;
    let mut dates = load(root, &name);
    dates.remove(&target);
    save(root, &name, &dates)?;
    Ok(status_of(&name, target, &dates))
}

#[cfg(test)]
mod tests {
    use super::{mark, run_ending, status, unmark, validate_name};
    use chrono::NaiveDate;
    use std::collections::BTreeSet;

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, day).expect("valid date")
    }

    fn dates(days: &[u32]) -> BTreeSet<NaiveDate> {
        days.iter().map(|day| date(*day)).collect()
    }

    #[test]
    fn a_run_ending_today_counts_today() {
        assert_eq!(run_ending(date(24), &dates(&[22, 23, 24])), 3);
    }

    #[test]
    fn an_undecided_today_still_counts_yesterdays_run() {
        // The user has not decided about tonight yet; yesterday's run is real.
        assert_eq!(run_ending(date(24), &dates(&[21, 22, 23])), 3);
    }

    #[test]
    fn a_gap_older_than_yesterday_is_a_broken_streak() {
        assert_eq!(run_ending(date(24), &dates(&[20, 21, 22])), 0);
    }

    #[test]
    fn a_gap_inside_the_run_stops_the_count() {
        assert_eq!(run_ending(date(24), &dates(&[20, 22, 23, 24])), 3);
    }

    #[test]
    fn nothing_marked_is_a_zero_streak() {
        assert_eq!(run_ending(date(24), &BTreeSet::new()), 0);
    }

    #[test]
    fn marking_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        mark(root, "late-work", date(23)).expect("mark");
        mark(root, "late-work", date(23)).expect("mark again");
        let state = mark(root, "late-work", date(24)).expect("mark");

        assert_eq!(state.streak, 2);
        assert_eq!(state.total_marked, 2);
        assert_eq!(state.last_marked.as_deref(), Some("2026-08-24"));
    }

    #[test]
    fn unmarking_is_idempotent_and_shortens_the_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        for day in [22, 23, 24] {
            mark(root, "late-work", date(day)).expect("mark");
        }

        unmark(root, "late-work", date(23)).expect("unmark");
        unmark(root, "late-work", date(23)).expect("unmark again");

        assert_eq!(
            status(root, "late-work", date(24)).expect("status").streak,
            1
        );
    }

    #[test]
    fn streaks_are_independent_of_each_other() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        mark(root, "late-work", date(24)).expect("mark");

        assert_eq!(
            status(root, "morning-run", date(24))
                .expect("status")
                .streak,
            0
        );
    }

    #[test]
    fn a_name_that_is_not_a_safe_file_name_is_refused() {
        for name in ["", "../escape", "with space", "sla/sh"] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
        assert!(validate_name("late-work_2").is_ok());
    }

    #[test]
    fn an_unreadable_state_file_reads_as_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("tasks/.streaks")).expect("dir");
        std::fs::write(super::state_path(root, "late-work"), "not json").expect("write");

        assert_eq!(
            status(root, "late-work", date(24)).expect("status").streak,
            0
        );
    }
}
