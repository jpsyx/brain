//! Pure formatters for the native mutators' output.
//!
//! Every line the CLI prints is built here, so the wording is a checked
//! contract rather than a scattering of `eprintln!`s.

use std::fmt::Write as _;

use crate::tasks::mutate::{
    assign::AssignResult, backlog::BacklogResult, defer::DeferResult, remove::RemoveResult,
    touch::TouchResult,
};
use crate::theme::Theme;

pub(super) fn removed(result: &RemoveResult, theme: Theme) -> String {
    format!(
        "{} {}  {}{}\n",
        theme.success("removed:"),
        theme.accent(&result.task_id),
        theme.value(&result.task_name),
        if result.was_habit {
            format!("  {}", theme.warning("(habit chain retired)"))
        } else {
            String::new()
        }
    )
}

pub(super) fn touched(result: &TouchResult, today: &str, theme: Theme) -> String {
    format!(
        "{} {}  {}\n  {} {} → {}\n",
        theme.success("touched:"),
        theme.accent(&result.task_id),
        theme.value(&result.task_name),
        theme.muted("last_touched:"),
        theme.muted(&result.previous),
        theme.value(today)
    )
}

pub(super) fn assigned(result: &AssignResult, theme: Theme) -> String {
    let previous = if result.previous.trim().is_empty() {
        "(unassigned)".to_owned()
    } else {
        result.previous.clone()
    };
    format!(
        "{} {}  {}\n  {} {} → {}\n",
        theme.success("assigned:"),
        theme.accent(&result.task_id),
        theme.value(&result.task_name),
        theme.muted("assigned_to:"),
        theme.muted(&previous),
        theme.value(&result.assigned_to)
    )
}

pub(super) fn parked(result: &BacklogResult, theme: Theme) -> String {
    if result.already {
        return format!(
            "{} {}  {}\n",
            theme.info("already parked:"),
            theme.accent(&result.task_id),
            theme.value(&result.task_name)
        );
    }
    let mut out = if result.restored {
        format!(
            "{} {}  {}\n  {}\n",
            theme.success("restored from backlog:"),
            theme.accent(&result.task_id),
            theme.value(&result.task_name),
            theme.muted("status: backlog → not_started (set a due date and priority next)")
        )
    } else {
        format!(
            "{} {}  {}\n  {}\n",
            theme.success("moved to backlog:"),
            theme.accent(&result.task_id),
            theme.value(&result.task_name),
            theme.muted(&format!(
                "status: {} → backlog (due date, start date, and hard deadline cleared)",
                result.previous_status
            ))
        )
    };
    if let (Some(project), false) = (&result.project, result.restored) {
        let _ = writeln!(
            out,
            "  {} {}",
            theme.warning("part of project"),
            theme.value(project)
        );
        let _ = writeln!(
            out,
            "  {}",
            theme.muted("confirm whether the whole project should follow")
        );
    }
    out
}

pub(super) fn deferred(result: &DeferResult, theme: Theme) -> String {
    let mut out = format!(
        "{} {}  {}\n  {} {} → {}\n",
        theme.success("deferred:"),
        theme.accent(&result.task_id),
        theme.value(&result.task_name),
        theme.muted("due_date:"),
        theme.muted(if result.old_due.trim().is_empty() {
            "(none)"
        } else {
            &result.old_due
        }),
        theme.value(&result.new_due)
    );
    match result.no_penalty_reason {
        Some(reason) => {
            let _ = writeln!(
                out,
                "  {} {}",
                theme.muted(&format!(
                    "defer_count: {} (unchanged —",
                    result.old_defer_count
                )),
                theme.muted(&format!("no-penalty defer, {reason})"))
            );
        }
        None => {
            let _ = writeln!(
                out,
                "  {} {} → {}",
                theme.muted("defer_count:"),
                theme.muted(&result.old_defer_count.to_string()),
                theme.value(&result.new_defer_count.to_string())
            );
        }
    }
    if result.dropped_mit {
        let _ = writeln!(
            out,
            "  {}",
            theme.muted("task_type: dropped `mit` (defer-demote)")
        );
    }
    if let Some((before, after)) = &result.demoted_priority {
        let _ = writeln!(
            out,
            "  {} {} → {}  {}",
            theme.muted("priority:"),
            theme.muted(before),
            theme.value(after),
            theme.muted("(defer-demote)")
        );
    }
    if !result.cascaded.is_empty() {
        let _ = writeln!(
            out,
            "  {} {}",
            theme.info(&format!(
                "cascaded {} later chunk(s)",
                result.cascaded.len()
            )),
            theme.muted("(due date only; defer_count untouched)")
        );
        for chunk in &result.cascaded {
            let _ = writeln!(
                out,
                "      {}  {} → {}",
                theme.accent(&chunk.task_id),
                theme.value(&chunk.task_name),
                theme.value(&chunk.due_date)
            );
        }
    }
    if result.no_penalty_reason.is_none() && result.new_defer_count >= 3 {
        let _ = writeln!(
            out,
            "  {}",
            theme
                .warning("deferred 3+ times — consider removing it or committing to a firmer date")
        );
    }
    if let Some(issue) = &result.linear_issue {
        let _ = writeln!(
            out,
            "  {} {} {}",
            theme.warning("LINEAR:"),
            theme.accent(issue),
            theme.muted("push the new due date (and any demoted priority) to the issue")
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::mutate::chunks::Cascaded;

    fn plain() -> Theme {
        Theme::dark(false)
    }

    fn base_defer() -> DeferResult {
        DeferResult {
            task_id: "T1".to_owned(),
            task_name: "Ship it".to_owned(),
            old_due: "2026-08-24".to_owned(),
            new_due: "2026-08-31".to_owned(),
            old_defer_count: 0,
            new_defer_count: 1,
            no_penalty_reason: None,
            dropped_mit: false,
            demoted_priority: None,
            cascaded: Vec::new(),
            linear_issue: None,
        }
    }

    #[test]
    fn a_penalised_defer_reports_the_new_count() {
        let out = deferred(&base_defer(), plain());
        assert!(out.contains("deferred: T1  Ship it"), "{out}");
        assert!(out.contains("due_date: 2026-08-24 → 2026-08-31"), "{out}");
        assert!(out.contains("defer_count: 0 → 1"), "{out}");
    }

    #[test]
    fn a_no_penalty_defer_says_why_the_count_did_not_move() {
        let result = DeferResult {
            no_penalty_reason: Some("waiting"),
            new_defer_count: 0,
            ..base_defer()
        };
        let out = deferred(&result, plain());
        assert!(out.contains("no-penalty defer, waiting"), "{out}");
        assert!(!out.contains("→ 1"), "{out}");
    }

    #[test]
    fn the_third_defer_warns() {
        let result = DeferResult {
            old_defer_count: 2,
            new_defer_count: 3,
            ..base_defer()
        };
        assert!(deferred(&result, plain()).contains("deferred 3+ times"));
    }

    #[test]
    fn a_cascade_lists_every_chunk_it_moved() {
        let result = DeferResult {
            cascaded: vec![Cascaded {
                task_id: "T2".to_owned(),
                task_name: "Ship it (2/3)".to_owned(),
                due_date: "2026-09-01".to_owned(),
            }],
            ..base_defer()
        };
        let out = deferred(&result, plain());
        assert!(out.contains("cascaded 1 later chunk(s)"), "{out}");
        assert!(out.contains("T2  Ship it (2/3) → 2026-09-01"), "{out}");
    }

    #[test]
    fn a_tracker_link_is_surfaced_because_the_binary_cannot_reach_it() {
        let result = DeferResult {
            linear_issue: Some("ENG-7".to_owned()),
            ..base_defer()
        };
        assert!(deferred(&result, plain()).contains("LINEAR: ENG-7"));
    }

    #[test]
    fn parking_names_the_project_that_may_have_to_follow() {
        let result = BacklogResult {
            task_id: "T1".to_owned(),
            task_name: "Ship it".to_owned(),
            previous_status: "in_progress".to_owned(),
            restored: false,
            already: false,
            project: Some("Website".to_owned()),
        };
        let out = parked(&result, plain());
        assert!(out.contains("moved to backlog: T1"), "{out}");
        assert!(out.contains("part of project Website"), "{out}");
    }

    #[test]
    fn retiring_a_habit_chain_is_called_out() {
        let result = RemoveResult {
            task_id: "H1".to_owned(),
            task_name: "Stretch".to_owned(),
            was_habit: true,
        };
        assert!(removed(&result, plain()).contains("(habit chain retired)"));
    }

    #[test]
    fn an_unassigned_row_reads_as_unassigned() {
        let result = AssignResult {
            task_id: "T1".to_owned(),
            task_name: "Ship it".to_owned(),
            previous: String::new(),
            assigned_to: "kristi".to_owned(),
        };
        assert!(assigned(&result, plain()).contains("(unassigned) → kristi"));
    }
}
