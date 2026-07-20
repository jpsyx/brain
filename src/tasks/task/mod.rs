//! Task data model + CSV loading. Mirrors `~/brain/tasks/SCHEMA.json`.
//!
//! This module owns the normalized [`Task`] struct and its predicates;
//! [`load`] owns the on-disk CSV row structs and the loaders that turn a
//! file into `Vec<Task>`.

mod load;

pub use load::{load_habits, load_tasks};

use chrono::NaiveDate;

/// Normalized, in-memory view of a task (or habit — same struct) with dates
/// parsed and pipe-lists split. Habit-only fields default to empty / zero
/// when loaded from tasks.csv.
#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub types: Vec<String>,
    pub status: String,
    pub priority: String,
    pub due_date: Option<NaiveDate>,
    pub hard_deadline: bool,
    pub start_date: Option<NaiveDate>,
    pub notes: String,
    pub project: String,
    pub energy: String,
    pub context: String,
    pub estimated_duration: Option<u32>,
    pub defer_count: u32,
    pub last_touched: Option<NaiveDate>,
    pub see_also: String,
    pub blocked_by: Vec<String>,
    pub completed_date: Option<NaiveDate>,
    /// Linear issue identifier (e.g. `AVA-123`) for tasks mirrored to
    /// Linear; empty for unlinked / non-code tasks (and always empty for
    /// habits, which never link to Linear).
    pub linear_issue: String,
}

impl Task {
    pub(crate) fn split_pipe(s: &str) -> Vec<String> {
        s.split('|')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect()
    }

    /// True iff the entry came from `habits.csv` — i.e. its canonical ID
    /// uses the `H` prefix.
    #[must_use]
    pub fn is_habit(&self) -> bool {
        self.id
            .chars()
            .next()
            .is_some_and(|c| c.eq_ignore_ascii_case(&'H'))
    }

    /// Whether this habit should appear in today's habits view. The rule
    /// is intentionally tight: show only habits that are due today or
    /// overdue AND not yet done. Future-dated habits (e.g. monthly
    /// recurrence that lands next week) are hidden until their cycle
    /// rolls around. Habits already completed for the current cycle are
    /// hidden too — the `/todo` machinery owns flipping `status` back to
    /// `not_started` and advancing `due_date` when the recurrence
    /// elapses.
    #[must_use]
    pub fn is_habit_due_today(&self, today: NaiveDate) -> bool {
        if self.is_done() {
            return false;
        }
        self.due_date.is_some_and(|d| d <= today)
    }

    #[must_use]
    pub fn is_done(&self) -> bool {
        self.status == "done"
    }

    /// True iff this row's `status` is `done` AND `completed_date` is
    /// `today`. Used by the startup-triage check to decide whether the
    /// configured daily-triage habit has already been completed for the
    /// current day. Note that habits recur, so a stale `done` row from
    /// yesterday will not match — only the current cycle counts.
    #[must_use]
    pub fn is_completed_today(&self, today: NaiveDate) -> bool {
        self.is_done() && self.completed_date == Some(today)
    }

    #[must_use]
    pub fn is_mit(&self) -> bool {
        self.types.iter().any(|t| t == "mit")
    }

    /// True iff this row is parked in the backlog (`status == "backlog"`).
    /// Backlog tasks are surfaced only in the Backlog and All views.
    #[must_use]
    pub fn is_backlog(&self) -> bool {
        self.status == "backlog"
    }

    #[must_use]
    pub fn is_past_due(&self, today: NaiveDate) -> bool {
        !self.is_done() && self.due_date.is_some_and(|d| d < today)
    }

    #[must_use]
    pub fn is_deferred(&self, today: NaiveDate) -> bool {
        self.start_date.is_some_and(|d| d > today)
    }

    #[must_use]
    pub fn is_stale(&self, today: NaiveDate) -> bool {
        !self.is_done()
            && self
                .last_touched
                .is_some_and(|d| (today - d).num_days() >= 21)
    }

    /// True iff this task carries a non-blank Linear issue identifier.
    #[must_use]
    pub fn has_linear(&self) -> bool {
        !self.linear_issue.trim().is_empty()
    }

    /// Full Linear issue URL for this task, or `None` when it carries no
    /// identifier. `base` is the workspace issue prefix (e.g.
    /// `https://linear.app/avandar/issue/`); the trimmed identifier is
    /// appended to it.
    #[must_use]
    pub fn linear_url(&self, base: &str) -> Option<String> {
        let id = self.linear_issue.trim();
        if id.is_empty() {
            return None;
        }
        Some(format!("{base}{id}"))
    }

    #[must_use]
    pub fn matches_search(&self, q: &str) -> bool {
        let q = q.to_ascii_lowercase();
        self.name.to_ascii_lowercase().contains(&q)
            || self.notes.to_ascii_lowercase().contains(&q)
            || self.project.to_ascii_lowercase().contains(&q)
            || self.id.to_ascii_lowercase().contains(&q)
    }
}

#[cfg(test)]
#[must_use]
pub fn test_task(id: &str, status: &str) -> Task {
    Task {
        id: id.to_owned(),
        name: format!("test task {id}"),
        types: Vec::new(),
        status: status.to_owned(),
        priority: "p2".to_owned(),
        due_date: None,
        hard_deadline: false,
        start_date: None,
        notes: String::new(),
        project: String::new(),
        energy: String::new(),
        context: String::new(),
        estimated_duration: None,
        defer_count: 0,
        last_touched: None,
        see_also: String::new(),
        blocked_by: Vec::new(),
        completed_date: None,
        linear_issue: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Task, test_task};
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    // --- Task predicates ---

    #[test]
    fn is_done_when_status_eq_done() {
        let mut t = test_task("T1", "not_started");
        assert!(!t.is_done());
        t.status = "done".to_owned();
        assert!(t.is_done());
    }

    #[test]
    fn is_backlog_iff_status_eq_backlog() {
        let mut t = test_task("T1", "not_started");
        assert!(!t.is_backlog());
        t.status = "backlog".to_owned();
        assert!(t.is_backlog());
    }

    #[test]
    fn is_mit_iff_types_contains_mit() {
        let mut t = test_task("T1", "not_started");
        assert!(!t.is_mit());
        t.types.push("mit".to_owned());
        assert!(t.is_mit());
    }

    #[test]
    fn is_past_due_requires_undone_and_past_due_date() {
        let today = d(2026, 6, 23);
        let mut t = test_task("T1", "not_started");
        t.due_date = Some(d(2026, 6, 20));
        assert!(t.is_past_due(today));
        // due_date == today is NOT past-due
        t.due_date = Some(today);
        assert!(!t.is_past_due(today));
        // done tasks are never past-due
        t.due_date = Some(d(2026, 6, 20));
        t.status = "done".to_owned();
        assert!(!t.is_past_due(today));
    }

    #[test]
    fn is_deferred_when_start_date_in_future() {
        let today = d(2026, 6, 23);
        let mut t = test_task("T1", "not_started");
        assert!(!t.is_deferred(today));
        t.start_date = Some(d(2026, 6, 24));
        assert!(t.is_deferred(today));
        t.start_date = Some(today);
        assert!(!t.is_deferred(today));
    }

    #[test]
    fn is_stale_requires_21_days_no_touch_and_not_done() {
        let today = d(2026, 6, 23);
        let mut t = test_task("T1", "not_started");
        t.last_touched = Some(today - chrono::Duration::days(20));
        assert!(!t.is_stale(today));
        t.last_touched = Some(today - chrono::Duration::days(21));
        assert!(t.is_stale(today));
        t.status = "done".to_owned();
        assert!(!t.is_stale(today));
    }

    #[test]
    fn is_habit_recognizes_h_prefix_case_insensitive() {
        assert!(test_task("H7", "not_started").is_habit());
        assert!(test_task("h31", "not_started").is_habit());
        assert!(!test_task("T1", "not_started").is_habit());
        assert!(!test_task("", "not_started").is_habit());
    }

    #[test]
    fn is_habit_due_today_excludes_done_and_future() {
        let today = d(2026, 6, 23);
        let mut h = test_task("H1", "not_started");
        h.due_date = Some(today);
        assert!(h.is_habit_due_today(today));
        // overdue counts
        h.due_date = Some(d(2026, 6, 20));
        assert!(h.is_habit_due_today(today));
        // future does not
        h.due_date = Some(d(2026, 6, 25));
        assert!(!h.is_habit_due_today(today));
        // done never does
        h.due_date = Some(today);
        h.status = "done".to_owned();
        assert!(!h.is_habit_due_today(today));
    }

    #[test]
    fn is_completed_today_requires_done_and_completed_date_eq_today() {
        let today = d(2026, 6, 23);
        let mut h = test_task("H31", "done");
        h.completed_date = Some(today);
        assert!(h.is_completed_today(today));
        // stale done row from yesterday's cycle: not today
        h.completed_date = Some(d(2026, 6, 22));
        assert!(!h.is_completed_today(today));
        // not done with today's date: not today
        h.completed_date = Some(today);
        h.status = "not_started".to_owned();
        assert!(!h.is_completed_today(today));
    }

    // --- Linear link ---

    #[test]
    fn has_linear_is_false_when_empty_or_whitespace() {
        let mut t = test_task("T1", "not_started");
        assert!(!t.has_linear());
        t.linear_issue = "   ".to_owned();
        assert!(!t.has_linear());
        t.linear_issue = "AVA-123".to_owned();
        assert!(t.has_linear());
    }

    #[test]
    fn linear_url_none_when_empty_or_whitespace() {
        let base = "https://linear.app/avandar/issue/";
        let mut t = test_task("T1", "not_started");
        assert_eq!(t.linear_url(base), None);
        t.linear_issue = "   ".to_owned();
        assert_eq!(t.linear_url(base), None);
    }

    #[test]
    fn linear_url_joins_base_and_trimmed_identifier() {
        let base = "https://linear.app/avandar/issue/";
        let mut t = test_task("T1", "not_started");
        t.linear_issue = "  AVA-123 ".to_owned();
        assert_eq!(
            t.linear_url(base).as_deref(),
            Some("https://linear.app/avandar/issue/AVA-123"),
        );
    }

    #[test]
    fn matches_search_is_case_insensitive_and_covers_multiple_fields() {
        let mut t = test_task("T123", "not_started");
        t.name = "Fill out PLTR spreadsheet".to_owned();
        t.notes = "see /finance/holdings".to_owned();
        t.project = "investing".to_owned();
        assert!(t.matches_search("PLTR"));
        assert!(t.matches_search("pltr")); // case-insensitive
        assert!(t.matches_search("invest")); // project
        assert!(t.matches_search("holdings")); // notes
        assert!(t.matches_search("t123")); // id, lowercased
        assert!(!t.matches_search("unrelated"));
    }

    // --- pipe splitting (used by types / blocked_by) ---

    #[test]
    fn split_pipe_skips_empty_segments_and_trims() {
        assert_eq!(
            Task::split_pipe("mit | code |  | finance"),
            vec!["mit".to_owned(), "code".to_owned(), "finance".to_owned()],
        );
        assert_eq!(Task::split_pipe(""), Vec::<String>::new());
    }
}
