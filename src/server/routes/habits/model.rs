//! Pure habit model for the ingress-scoped habits route.
//!
//! Owns the on-disk CSV row, the normalized [`Habit`], the pure
//! filter-and-sort [`classify`] (mirroring the Python `read_today_habits`),
//! and the time-of-day / priority ordering the view groups by. The only IO
//! is [`load`], a thin reader; every decision lives in a pure function so it
//! can be unit-tested against hand-built rows.

use std::path::Path;

use chrono::NaiveDate;
use serde::Deserialize;

/// Priority buckets, most urgent first. A habit whose `priority` is not one of
/// these sorts after all of them (mirrors the Python `99` fallback).
pub const PRIORITY_ORDER: [&str; 5] = ["p0", "p1", "p2", "p3", "p4"];

/// Position of `priority` in [`PRIORITY_ORDER`], or `99` if unknown.
#[must_use]
pub fn priority_index(priority: &str) -> usize {
    PRIORITY_ORDER
        .iter()
        .position(|p| *p == priority)
        .unwrap_or(99)
}

/// Time-of-day bucket a habit falls into, ordered morning → anytime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeBucket {
    Morning,
    Afternoon,
    Evening,
    Anytime,
}

impl TimeBucket {
    /// Buckets in display / sort order.
    pub const ALL: [Self; 4] = [Self::Morning, Self::Afternoon, Self::Evening, Self::Anytime];

    /// Sort rank (0 = earliest).
    #[must_use]
    pub fn order(self) -> u8 {
        match self {
            Self::Morning => 0,
            Self::Afternoon => 1,
            Self::Evening => 2,
            Self::Anytime => 3,
        }
    }

    /// Human label for the section header.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Morning => "Morning",
            Self::Afternoon => "Afternoon",
            Self::Evening => "Evening",
            Self::Anytime => "Anytime",
        }
    }
}

/// Minutes since midnight for an `ideal_time` like `9:00 AM`, `6:30 AM`,
/// `2:15 PM`, `12:00 PM`, or the hour-only `9 AM`. Case- and dot-tolerant.
/// Returns `None` for blank or unparseable input.
#[must_use]
pub fn parse_ideal_minutes(raw: &str) -> Option<u32> {
    let s = raw.trim().to_ascii_uppercase().replace('.', "");
    if s.is_empty() {
        return None;
    }
    let (time_part, meridiem) = s.rsplit_once(' ')?;
    let pm = match meridiem {
        "AM" => false,
        "PM" => true,
        _ => return None,
    };
    let (h, m) = match time_part.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (time_part.parse::<u32>().ok()?, 0),
    };
    if h == 0 || h > 12 || m > 59 {
        return None;
    }
    let hour24 = match (h, pm) {
        (12, false) => 0,    // 12 AM = midnight
        (12, true) => 12,    // 12 PM = noon
        (h, false) => h,     // AM
        (h, true) => h + 12, // PM
    };
    Some(hour24 * 60 + m)
}

/// The time bucket for an `ideal_time` string (`None`/blank → [`TimeBucket::Anytime`]).
#[must_use]
pub fn time_bucket(ideal_time: Option<&str>) -> TimeBucket {
    match ideal_time.and_then(parse_ideal_minutes) {
        None => TimeBucket::Anytime,
        Some(m) if m < 12 * 60 => TimeBucket::Morning,
        Some(m) if m < 17 * 60 + 30 => TimeBucket::Afternoon,
        Some(_) => TimeBucket::Evening,
    }
}

/// A single habit, normalized for filtering, sorting, and rendering.
#[derive(Debug, Clone)]
pub struct Habit {
    pub task_id: String,
    pub name: String,
    pub status: String,
    pub priority: String,
    pub due_date: Option<NaiveDate>,
    pub hard_deadline: bool,
    pub estimated_duration: Option<u32>,
    pub ideal_time: Option<String>,
    pub notes: String,
    pub completed_date: Option<NaiveDate>,
}

impl Habit {
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.status == "done"
    }

    #[must_use]
    pub fn bucket(&self) -> TimeBucket {
        time_bucket(self.ideal_time.as_deref())
    }

    #[must_use]
    pub fn ideal_minutes(&self) -> Option<u32> {
        self.ideal_time.as_deref().and_then(parse_ideal_minutes)
    }
}

/// Split today's habits into `(pending, completed_today)`.
///
/// `pending` = not done AND (no due date OR due on/before `today`), sorted by
/// time bucket → priority → ideal-time minute → duration → name.
/// `completed_today` = done AND completed on `today`, sorted by name.
#[must_use]
pub fn classify(rows: Vec<Habit>, today: NaiveDate) -> (Vec<Habit>, Vec<Habit>) {
    let mut pending = Vec::new();
    let mut completed = Vec::new();
    for h in rows {
        if h.is_done() {
            if h.completed_date == Some(today) {
                completed.push(h);
            }
            continue;
        }
        if h.due_date.is_none_or(|d| d <= today) {
            pending.push(h);
        }
    }
    pending.sort_by_key(sort_key);
    completed.sort_by_key(|h| h.name.to_lowercase());
    (pending, completed)
}

/// Sort key for a pending habit: time bucket → priority → ideal-time minute →
/// duration → name. Missing minute/duration sort last (the `9999` sentinel).
fn sort_key(h: &Habit) -> (u8, usize, u32, u32, String) {
    (
        h.bucket().order(),
        priority_index(&h.priority),
        h.ideal_minutes().unwrap_or(9999),
        h.estimated_duration.unwrap_or(9999),
        h.name.to_lowercase(),
    )
}

/// The subset of `habits.csv` columns the habits view needs. The csv reader
/// matches these against the header by name, so the remaining columns
/// (`recur_*`, `created_date`, …) are read past and ignored.
#[derive(Debug, Deserialize)]
struct HabitCsvRow {
    task_id: String,
    task_name: String,
    status: String,
    priority: String,
    due_date: String,
    hard_deadline: String,
    notes: String,
    estimated_duration: String,
    ideal_time: String,
    completed_date: String,
}

/// Parse a `YYYY-MM-DD` (or `YYYY-MM-DDTHH:MM`) field into a date, `None` if
/// blank or malformed.
fn parse_date(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let date_part = s.split('T').next().unwrap_or(s);
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
}

impl From<HabitCsvRow> for Habit {
    fn from(row: HabitCsvRow) -> Self {
        let priority = if row.priority.trim().is_empty() {
            "p4".to_owned()
        } else {
            row.priority
        };
        let ideal = row.ideal_time.trim();
        Self {
            task_id: row.task_id,
            name: row.task_name,
            status: row.status,
            priority,
            due_date: parse_date(&row.due_date),
            hard_deadline: row.hard_deadline.eq_ignore_ascii_case("true"),
            estimated_duration: row.estimated_duration.trim().parse().ok(),
            ideal_time: (!ideal.is_empty()).then(|| ideal.to_owned()),
            notes: row.notes,
            completed_date: parse_date(&row.completed_date),
        }
    }
}

/// Load `<root>/tasks/habits.csv` into normalized [`Habit`]s. A missing or
/// unreadable file yields an empty list rather than an error, so the page
/// degrades gracefully on setups without habits.
#[must_use]
pub fn load(root: &Path) -> Vec<Habit> {
    let path = root.join("tasks").join("habits.csv");
    let Ok(mut rdr) = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(&path)
    else {
        return Vec::new();
    };
    rdr.deserialize::<HabitCsvRow>()
        .filter_map(Result::ok)
        .map(Habit::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn habit(id: &str, status: &str, priority: &str) -> Habit {
        Habit {
            task_id: id.to_owned(),
            name: format!("habit {id}"),
            status: status.to_owned(),
            priority: priority.to_owned(),
            due_date: None,
            hard_deadline: false,
            estimated_duration: None,
            ideal_time: None,
            notes: String::new(),
            completed_date: None,
        }
    }

    #[test]
    fn classify_keeps_undated_pending_habit() {
        let today = d(2026, 7, 25);
        let (pending, completed) = classify(vec![habit("H1", "not_started", "p2")], today);
        assert_eq!(pending.len(), 1);
        assert!(completed.is_empty());
    }

    #[test]
    fn classify_excludes_future_due_habit() {
        let today = d(2026, 7, 25);
        let mut h = habit("H1", "not_started", "p2");
        h.due_date = Some(d(2026, 8, 1));
        let (pending, _) = classify(vec![h], today);
        assert!(pending.is_empty(), "a future-due habit must be hidden");
    }

    #[test]
    fn classify_keeps_overdue_habit_pending() {
        let today = d(2026, 7, 25);
        let mut h = habit("H1", "not_started", "p2");
        h.due_date = Some(d(2026, 7, 20));
        let (pending, _) = classify(vec![h], today);
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn classify_puts_done_today_in_completed() {
        let today = d(2026, 7, 25);
        let mut h = habit("H1", "done", "p2");
        h.completed_date = Some(today);
        let (pending, completed) = classify(vec![h], today);
        assert!(pending.is_empty());
        assert_eq!(completed.len(), 1);
    }

    #[test]
    fn classify_hides_done_habit_from_a_prior_day() {
        let today = d(2026, 7, 25);
        let mut h = habit("H1", "done", "p2");
        h.completed_date = Some(d(2026, 7, 24));
        let (pending, completed) = classify(vec![h], today);
        assert!(pending.is_empty());
        assert!(completed.is_empty(), "yesterday's completion must not show");
    }

    #[test]
    fn classify_orders_by_time_bucket_then_priority() {
        let today = d(2026, 7, 25);
        // An afternoon p0 should still sort AFTER a morning p2 (bucket wins).
        let mut morning = habit("H_morning", "not_started", "p2");
        morning.ideal_time = Some("9:00 AM".to_owned());
        let mut afternoon = habit("H_afternoon", "not_started", "p0");
        afternoon.ideal_time = Some("2:00 PM".to_owned());
        let (pending, _) = classify(vec![afternoon, morning], today);
        assert_eq!(
            pending
                .iter()
                .map(|h| h.task_id.as_str())
                .collect::<Vec<_>>(),
            ["H_morning", "H_afternoon"],
        );
    }

    #[test]
    fn classify_orders_by_priority_within_a_bucket() {
        let today = d(2026, 7, 25);
        // Same (anytime) bucket → priority ascending (p0 before p3).
        let a = habit("H_low", "not_started", "p3");
        let b = habit("H_high", "not_started", "p0");
        let (pending, _) = classify(vec![a, b], today);
        assert_eq!(
            pending
                .iter()
                .map(|h| h.task_id.as_str())
                .collect::<Vec<_>>(),
            ["H_high", "H_low"],
        );
    }

    #[test]
    fn parse_ideal_minutes_handles_common_forms() {
        assert_eq!(parse_ideal_minutes("9:00 AM"), Some(9 * 60));
        assert_eq!(parse_ideal_minutes("6:30 am"), Some(6 * 60 + 30));
        assert_eq!(parse_ideal_minutes("2:15 PM"), Some(14 * 60 + 15));
        assert_eq!(parse_ideal_minutes("12:00 PM"), Some(12 * 60));
        assert_eq!(parse_ideal_minutes("12:00 AM"), Some(0));
        assert_eq!(parse_ideal_minutes("9 AM"), Some(9 * 60));
        assert_eq!(parse_ideal_minutes(""), None);
        assert_eq!(parse_ideal_minutes("noonish"), None);
    }

    #[test]
    fn load_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_empty());
    }

    #[test]
    fn load_parses_columns_ignoring_extra_ones() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("tasks")).unwrap();
        // Full habits.csv header (order + extra columns the model doesn't use).
        std::fs::write(
            dir.path().join("tasks").join("habits.csv"),
            "task_id,task_name,status,priority,due_date,hard_deadline,assignee,\
see_also,notes,project,energy_level,context,estimated_duration,ideal_time,\
recur_interval,recur_unit,created_date,completed_date\n\
H1,Floss,not_started,p2,2026-07-25,false,pablo,,nightly,,,,8,8:45 AM,1,days,2026-01-07,\n",
        )
        .unwrap();
        let rows = load(dir.path());
        assert_eq!(rows.len(), 1);
        let h = &rows[0];
        assert_eq!(h.task_id, "H1");
        assert_eq!(h.name, "Floss");
        assert_eq!(h.priority, "p2");
        assert_eq!(h.estimated_duration, Some(8));
        assert_eq!(h.ideal_time.as_deref(), Some("8:45 AM"));
        assert_eq!(h.due_date, Some(d(2026, 7, 25)));
        assert_eq!(h.notes, "nightly");
    }

    #[test]
    fn time_bucket_boundaries() {
        assert_eq!(time_bucket(Some("11:59 AM")), TimeBucket::Morning);
        assert_eq!(time_bucket(Some("12:00 PM")), TimeBucket::Afternoon);
        assert_eq!(time_bucket(Some("5:29 PM")), TimeBucket::Afternoon);
        assert_eq!(time_bucket(Some("5:30 PM")), TimeBucket::Evening);
        assert_eq!(time_bucket(None), TimeBucket::Anytime);
    }
}
