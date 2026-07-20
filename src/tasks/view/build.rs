//! The pipeline that materializes a [`ViewSpec`] from a raw `Vec<Task>`:
//! selector + per-view predicate + CLI filters, then sort, then titles.

use chrono::{Datelike, Duration, NaiveDate};

use crate::tasks::cli::{Cli, Filters};
use crate::tasks::selector::{self, Selector};
use crate::tasks::task::Task;

use super::sort::sort_tasks;
use super::{View, ViewSpec, view_filter};

#[must_use]
pub fn build_view(
    cli: &Cli,
    selector: &Selector,
    active_view: Option<View>,
    all_tasks: Vec<Task>,
    today: NaiveDate,
) -> ViewSpec {
    let total = all_tasks.len();

    let mut filtered: Vec<Task> = all_tasks
        .into_iter()
        .filter(|t| selector::matches(selector, t, today))
        .filter(|t| active_view.is_none_or(|v| view_filter(v, t, today)))
        .filter(|t| keeps_visibility(t, &cli.filters, today))
        .filter(|t| keeps_filters(t, &cli.filters, today))
        .collect();

    sort_tasks(&mut filtered, &cli.display.sort);
    if cli.display.reverse {
        filtered.reverse();
    }

    let (mut title, subtitle) = active_view
        .map_or_else(|| selector::titles(selector, today), |v| view_titles(v, today));
    if let Some(q) = &cli.filters.search {
        title = format!("{title} · search '{q}'");
    }
    ViewSpec {
        title,
        subtitle,
        tasks: filtered,
        total,
    }
}

/// Human-readable title/subtitle for a view. Today/Week reuse the
/// selector-driven labels; Mit/PastDue/All get their own copy so the
/// header reflects the view, not the underlying `Selector::All`.
fn view_titles(view: View, today: NaiveDate) -> (String, String) {
    match view {
        View::Today => selector::titles(&Selector::Today, today),
        View::Week => {
            let dow = i64::from(today.weekday().num_days_from_monday());
            let monday = today - Duration::days(dow);
            selector::titles(&Selector::Week(monday), today)
        }
        View::Mit => ("MIT".to_owned(), "Most Important Tasks".to_owned()),
        View::PastDue => (
            "Past due".to_owned(),
            format!("not done, due before {today}"),
        ),
        View::Habits => ("Habits".to_owned(), format!("due today ({today})")),
        View::Backlog => ("Backlog".to_owned(), "parked indefinitely".to_owned()),
        View::All => ("All tasks".to_owned(), String::new()),
    }
}

/// "Visibility" = the show-by-default rules (hide done + deferred unless opt-in).
fn keeps_visibility(t: &Task, f: &Filters, today: NaiveDate) -> bool {
    (f.include_done || !t.is_done()) && (f.include_deferred || !t.is_deferred(today))
}

/// All explicit `--flag` filters. Returns true when the task should be kept.
fn keeps_filters(t: &Task, f: &Filters, today: NaiveDate) -> bool {
    f.hard_deadline.is_none_or(|want| t.hard_deadline == want)
        && f.status
            .as_deref()
            .is_none_or(|w| t.status.eq_ignore_ascii_case(w))
        && f.priority
            .as_deref()
            .is_none_or(|w| t.priority.eq_ignore_ascii_case(w))
        && f.task_type
            .as_deref()
            .is_none_or(|w| t.types.iter().any(|x| x.eq_ignore_ascii_case(w)))
        && f.project
            .as_deref()
            .is_none_or(|w| t.project.eq_ignore_ascii_case(w))
        && f.energy
            .as_deref()
            .is_none_or(|w| t.energy.eq_ignore_ascii_case(w))
        && f.context
            .as_deref()
            .is_none_or(|w| t.context.eq_ignore_ascii_case(w))
        && (!f.past_due || t.is_past_due(today))
        && (!f.mit || t.is_mit())
        && (!f.stale || t.is_stale(today))
        && (!f.no_due || t.due_date.is_none())
        && (!f.blocked || !t.blocked_by.is_empty())
        && f.search.as_deref().is_none_or(|q| t.matches_search(q))
}

#[cfg(test)]
mod tests {
    use super::build_view;
    use crate::tasks::cli::Cli;
    use crate::tasks::selector::Selector;
    use crate::tasks::task::test_task;
    use crate::tasks::view::View;
    use chrono::NaiveDate;
    use clap::Parser;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn empty_cli() -> Cli {
        // Parsing an empty arg list gives us the all-defaults Cli (no
        // filters, no display flags, no search). Cleaner than handrolling
        // defaults for every flatten group.
        Cli::parse_from(["tasks"])
    }

    #[test]
    fn done_tasks_are_hidden_by_default_and_shown_when_included() {
        let today = d(2026, 6, 23);
        let mut done = test_task("T1", "done");
        done.due_date = Some(today);
        let mut open = test_task("T2", "not_started");
        open.due_date = Some(today);

        let cli = empty_cli();
        let view = build_view(&cli, &Selector::All, None, vec![done.clone(), open.clone()], today);
        assert_eq!(view.tasks.iter().map(|t| t.id.clone()).collect::<Vec<_>>(), vec!["T2"]);

        let mut cli2 = empty_cli();
        cli2.filters.include_done = true;
        let view2 = build_view(&cli2, &Selector::All, None, vec![done, open], today);
        // Both kept; order is priority-then-due-then-id, so T1 first.
        assert_eq!(view2.tasks.len(), 2);
    }

    #[test]
    fn view_filter_intersects_with_selector_and_cli_filters() {
        // Two MIT past-due, one MIT future, one non-MIT past-due.
        // View::Mit + selector::Today should produce both MIT past-due
        // (today's Today selector includes past-due undone).
        let today = d(2026, 6, 23);
        let mk = |id: &str, mit: bool, due: NaiveDate| {
            let mut t = test_task(id, "not_started");
            t.due_date = Some(due);
            if mit {
                t.types.push("mit".to_owned());
            }
            t
        };
        let tasks = vec![
            mk("T1", true, d(2026, 6, 20)),
            mk("T2", true, d(2026, 6, 21)),
            mk("T3", true, d(2026, 7, 10)),
            mk("T4", false, d(2026, 6, 20)),
        ];
        let cli = empty_cli();
        let view = build_view(&cli, &Selector::Today, Some(View::Mit), tasks, today);
        let ids: Vec<String> = view.tasks.iter().map(|t| t.id.clone()).collect();
        assert!(ids.contains(&"T1".to_owned()));
        assert!(ids.contains(&"T2".to_owned()));
        assert!(!ids.contains(&"T3".to_owned()));
        assert!(!ids.contains(&"T4".to_owned()));
    }

    #[test]
    fn sort_by_due_orders_earliest_first() {
        let today = d(2026, 6, 23);
        let mut a = test_task("T1", "not_started");
        a.due_date = Some(d(2026, 7, 10));
        let mut b = test_task("T2", "not_started");
        b.due_date = Some(d(2026, 6, 25));

        let mut cli = empty_cli();
        cli.display.sort = "due".to_owned();
        let view = build_view(&cli, &Selector::All, None, vec![a, b], today);
        let ids: Vec<String> = view.tasks.iter().map(|t| t.id.clone()).collect();
        assert_eq!(ids, vec!["T2", "T1"]);
    }

    #[test]
    fn reverse_flag_reverses_post_sort() {
        let today = d(2026, 6, 23);
        let a = test_task("T1", "not_started"); // p2
        let mut b = test_task("T2", "not_started");
        b.priority = "p0".to_owned();
        let mut cli = empty_cli();
        cli.display.reverse = true;
        let view = build_view(&cli, &Selector::All, None, vec![a, b], today);
        // Normal priority sort: p0 first → reversed: p2 first → T1 first.
        let ids: Vec<String> = view.tasks.iter().map(|t| t.id.clone()).collect();
        assert_eq!(ids, vec!["T1", "T2"]);
    }
}
