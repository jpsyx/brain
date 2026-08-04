//! The `tasks` shell's named views and the pipeline that materializes one.
//!
//! A [`View`] is which named view is active (today / mit / past_due /
//! week / habits / backlog / all); Tab in the event loop cycles through
//! [`View::CYCLE`]. [`build_view`] turns a raw `Vec<Task>` + CLI into a
//! [`ViewSpec`] (the title/subtitle/list the renderer draws). Views that
//! aren't a pure `Selector` (`Mit`, `PastDue`) layer their extra filter
//! on top via [`view_filter`].
//!
//! Submodules:
//! - [`build`] — the filter → sort → titles pipeline ([`build_view`]).
//! - [`sort`] — the `--sort` strategies and the priority-rank key.

mod build;
mod sort;

pub(crate) use build::apply_assignment_filter;
pub use build::build_view;

use chrono::{Datelike, Duration, NaiveDate};

use crate::tasks::selector::Selector;
use crate::tasks::task::Task;

/// Which named view is active. The initial view comes from the
/// positional token if it matches one (`today`, `mit`, `past_due`,
/// `week`, `all`) or defaults to `Today` for empty input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Today,
    Mit,
    PastDue,
    Week,
    Habits,
    Backlog,
    All,
}

impl View {
    pub const CYCLE: [Self; 7] = [
        Self::Today,
        Self::Mit,
        Self::PastDue,
        Self::Week,
        Self::Habits,
        Self::Backlog,
        Self::All,
    ];

    #[must_use]
    pub fn next(self) -> Self {
        let n = Self::CYCLE.len();
        let i = Self::CYCLE.iter().position(|v| *v == self).unwrap_or(0);
        Self::CYCLE[(i + 1) % n]
    }

    #[must_use]
    pub fn prev(self) -> Self {
        let n = Self::CYCLE.len();
        let i = Self::CYCLE.iter().position(|v| *v == self).unwrap_or(0);
        Self::CYCLE[(i + n - 1) % n]
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Mit => "mit",
            Self::PastDue => "past_due",
            Self::Week => "week",
            Self::Habits => "habits",
            Self::Backlog => "backlog",
            Self::All => "all",
        }
    }

    #[must_use]
    pub fn from_token(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "today" => Some(Self::Today),
            "mit" => Some(Self::Mit),
            "past_due" | "past-due" | "pastdue" | "overdue" => Some(Self::PastDue),
            "week" | "this-week" | "this_week" => Some(Self::Week),
            "habits" | "habit" => Some(Self::Habits),
            "backlog" => Some(Self::Backlog),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    #[must_use]
    pub fn selector(self, today: NaiveDate) -> Selector {
        match self {
            Self::Today => Selector::Today,
            Self::Week => {
                let dow = i64::from(today.weekday().num_days_from_monday());
                Selector::Week(today - Duration::days(dow))
            }
            // Mit / PastDue / All / Habits / Backlog scan every entry; their
            // narrowing happens through `view_filter` rather than the
            // selector. Habits also swap the underlying data source from
            // tasks.csv to habits.csv at the App layer.
            Self::Mit | Self::PastDue | Self::All | Self::Habits | Self::Backlog => Selector::All,
        }
    }
}

/// Extra per-view predicate applied on top of the selector + cli filters.
/// Returns `true` when the task should be kept.
#[must_use]
pub fn view_filter(view: View, t: &Task, today: NaiveDate) -> bool {
    // Backlog tasks are parked indefinitely: surface them only in the
    // dedicated Backlog view and the catch-all All view; never in any
    // active view (Today / Mit / PastDue / Week / Habits).
    if t.is_backlog() {
        return matches!(view, View::Backlog | View::All);
    }
    match view {
        View::Mit => t.is_mit(),
        View::PastDue => t.is_past_due(today),
        View::Habits => t.is_habit_due_today(today),
        // Only backlog tasks belong in the Backlog view; the early return
        // above already kept those, so every non-backlog task is dropped.
        View::Backlog => false,
        _ => true,
    }
}

pub struct ViewSpec {
    pub title: String,
    pub subtitle: String,
    pub tasks: Vec<Task>,
    pub total: usize,
}

#[cfg(test)]
mod tests {
    use super::{View, view_filter};
    use crate::tasks::task::test_task;
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn next_wraps_around_at_end() {
        // CYCLE: Today, Mit, PastDue, Week, Habits, Backlog, All
        assert_eq!(View::All.next(), View::Today);
        assert_eq!(View::Today.next(), View::Mit);
    }

    #[test]
    fn prev_wraps_around_at_start() {
        assert_eq!(View::Today.prev(), View::All);
        assert_eq!(View::Mit.prev(), View::Today);
    }

    #[test]
    fn backlog_sits_between_habits_and_all_in_cycle() {
        assert_eq!(View::Habits.next(), View::Backlog);
        assert_eq!(View::Backlog.next(), View::All);
        assert_eq!(View::All.prev(), View::Backlog);
        assert_eq!(View::Backlog.prev(), View::Habits);
    }

    #[test]
    fn backlog_view_shows_only_backlog_tasks() {
        let today = d(2026, 6, 23);
        let mut parked = test_task("T1", "backlog");
        parked.types.push("mit".to_owned());
        let open = test_task("T2", "not_started");
        assert!(view_filter(View::Backlog, &parked, today));
        assert!(!view_filter(View::Backlog, &open, today));
    }

    #[test]
    fn backlog_tasks_are_hidden_from_active_views_but_shown_in_all() {
        let today = d(2026, 6, 23);
        let mut parked = test_task("T1", "backlog");
        // Even an MIT-tagged backlog task stays out of the MIT view.
        parked.types.push("mit".to_owned());
        parked.due_date = Some(d(2026, 6, 20));
        for v in [
            View::Today,
            View::Mit,
            View::PastDue,
            View::Week,
            View::Habits,
        ] {
            assert!(!view_filter(v, &parked, today), "{v:?} should hide backlog");
        }
        assert!(view_filter(View::All, &parked, today));
    }

    #[test]
    fn from_token_handles_aliases_and_case() {
        assert_eq!(View::from_token("PAST-DUE"), Some(View::PastDue));
        assert_eq!(View::from_token("past_due"), Some(View::PastDue));
        assert_eq!(View::from_token("overdue"), Some(View::PastDue));
        assert_eq!(View::from_token("habit"), Some(View::Habits));
        assert_eq!(View::from_token("BACKLOG"), Some(View::Backlog));
        assert_eq!(View::from_token("this-week"), Some(View::Week));
        assert_eq!(View::from_token("nope"), None);
    }

    #[test]
    fn mit_filter_keeps_only_mit_tasks() {
        let today = d(2026, 6, 23);
        let mut t = test_task("T1", "not_started");
        assert!(!view_filter(View::Mit, &t, today));
        t.types.push("mit".to_owned());
        assert!(view_filter(View::Mit, &t, today));
    }

    #[test]
    fn past_due_filter_requires_undone_past_date() {
        let today = d(2026, 6, 23);
        let mut t = test_task("T1", "not_started");
        t.due_date = Some(d(2026, 6, 20));
        assert!(view_filter(View::PastDue, &t, today));
        t.status = "done".to_owned();
        assert!(!view_filter(View::PastDue, &t, today));
    }

    #[test]
    fn habits_filter_excludes_future_and_done() {
        let today = d(2026, 6, 23);
        let mut h = test_task("H1", "not_started");
        h.due_date = Some(today);
        assert!(view_filter(View::Habits, &h, today));
        h.due_date = Some(d(2026, 6, 25));
        assert!(!view_filter(View::Habits, &h, today));
    }

    #[test]
    fn all_today_week_keep_everything_through_view_filter() {
        let today = d(2026, 6, 23);
        let t = test_task("T1", "not_started");
        assert!(view_filter(View::All, &t, today));
        assert!(view_filter(View::Today, &t, today));
        assert!(view_filter(View::Week, &t, today));
    }
}
