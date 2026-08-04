//! Task card composition: each task renders to 3-5 short `Line`s (header
//! chip · name · meta · notes · see-also / linear), and
//! [`build_body_lines_with_ranges`] stitches a list of cards into the body
//! plus the per-task line ranges the tasks shell highlights.

use chrono::NaiveDate;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::tasks::task::Task;

use super::markdown::notes_lines;
use super::style::{
    ACCENT_CYAN, ACCENT_PURPLE, DUE_OVERDUE, DUE_TODAY, STATUS_DONE, TEXT_DIM, TEXT_PRIMARY,
    accent, dim, due_span, energy_icon, priority_style, sep, status_label, status_style, truncate,
    type_label, very_dim,
};

fn header_chip_line(
    task: &Task,
    tag_styles: &crate::personalization::tags::TagStyles,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(8);
    spans.push(accent(&task.priority));
    spans.push(Span::styled(
        format!("{:<5}", task.id),
        Style::new().fg(ACCENT_PURPLE).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        task.priority.to_ascii_uppercase(),
        priority_style(&task.priority),
    ));

    if !task.types.is_empty() {
        spans.push(sep());
        let labels: Vec<String> = task
            .types
            .iter()
            .map(|t| type_label(tag_styles, t))
            .collect();
        spans.push(Span::styled(labels.join(" · "), Style::new().fg(TEXT_DIM)));
    }
    if task.hard_deadline {
        spans.push(sep());
        spans.push(Span::styled(
            "⚠ hard",
            Style::new().fg(DUE_OVERDUE).add_modifier(Modifier::BOLD),
        ));
    }
    if task.defer_count >= 3 {
        spans.push(sep());
        spans.push(Span::styled(
            format!("↻ deferred ×{}", task.defer_count),
            Style::new().fg(DUE_TODAY),
        ));
    }
    if task.has_linear() {
        spans.push(sep());
        // Compact, clearly-Linear marker: the issue identifier only (never
        // the long URL — that blows out the row width). Dimmed so it reads
        // as metadata, not a call to action.
        spans.push(Span::styled(
            format!("◇ {}", task.linear_issue.trim()),
            Style::new().fg(TEXT_DIM),
        ));
    }
    Line::from(spans)
}

fn name_line(task: &Task) -> Line<'static> {
    let style = if task.is_done() {
        Style::new()
            .fg(STATUS_DONE)
            .add_modifier(Modifier::CROSSED_OUT)
    } else {
        Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD)
    };
    Line::from(vec![
        accent(&task.priority),
        Span::styled(task.name.clone(), style),
    ])
}

fn meta_line(task: &Task, today: NaiveDate, show_assignment: bool) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(12);
    spans.push(accent(&task.priority));
    spans.push(due_span(task.due_date, today, task.is_done()));
    spans.push(sep());
    spans.push(Span::styled(
        status_label(&task.status),
        status_style(&task.status),
    ));

    if show_assignment {
        let assigned_to = if task.assigned_to.trim().is_empty() {
            "unassigned"
        } else {
            task.assigned_to.trim()
        };
        spans.push(sep());
        spans.push(dim(format!("assigned to {assigned_to}")));
    }

    if !task.project.is_empty() {
        spans.push(sep());
        spans.push(Span::styled(
            format!("📁 {}", task.project),
            Style::new().fg(ACCENT_CYAN),
        ));
    }
    if !task.context.is_empty() {
        spans.push(sep());
        spans.push(Span::styled(
            format!("@{}", task.context),
            Style::new().fg(ACCENT_PURPLE),
        ));
    }
    if !task.energy.is_empty() {
        spans.push(sep());
        spans.push(dim(format!(
            "{} {}",
            energy_icon(&task.energy),
            task.energy
        )));
    }
    if let Some(mins) = task.estimated_duration {
        spans.push(sep());
        spans.push(dim(format!("⏱ {mins}m")));
    }
    if !task.blocked_by.is_empty() {
        spans.push(sep());
        spans.push(Span::styled(
            format!("🚧 blocked by {}", task.blocked_by.join(", ")),
            Style::new().fg(DUE_OVERDUE),
        ));
    }
    Line::from(spans)
}

fn see_also_line(task: &Task) -> Option<Line<'static>> {
    let trimmed = task.see_also.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(Line::from(vec![
        accent(&task.priority),
        very_dim("↪ "),
        Span::styled(truncate(trimmed, 110), Style::new().fg(ACCENT_CYAN)),
    ]))
}

#[must_use]
pub fn task_lines(
    task: &Task,
    today: NaiveDate,
    notes_expanded: bool,
    show_assignment: bool,
    tag_styles: &crate::personalization::tags::TagStyles,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(6);
    lines.push(header_chip_line(task, tag_styles));
    lines.push(name_line(task));
    lines.push(meta_line(task, today, show_assignment));
    lines.extend(notes_lines(task, notes_expanded));
    if let Some(l) = see_also_line(task) {
        lines.push(l);
    }
    // The compact `◇ <identifier>` marker in the header chip is the single
    // Linear indicator; the full URL is reachable via Ctrl+O. We deliberately
    // don't repeat it as a separate line in the expanded view (that read as a
    // duplicate of the header marker).
    lines.push(Line::from(""));
    lines
}

/// Body lines + the line-range each task occupies (excluding the trailing
/// blank separator).
///
/// The tasks shell uses the ranges for task-level selection highlighting and
/// scroll-into-view. Returns `(lines, empty)` when there are no tasks and the
/// caller already supplied an empty-state body.
#[must_use]
pub fn build_body_lines_with_ranges(
    tasks: &[Task],
    today: NaiveDate,
    show_assignment: bool,
    tag_styles: &crate::personalization::tags::TagStyles,
    is_expanded: impl Fn(&Task) -> bool,
) -> (Vec<Line<'static>>, Vec<std::ops::Range<usize>>) {
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(tasks.len() * 5 + 1);
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::with_capacity(tasks.len());
    lines.push(Line::from(""));
    for task in tasks {
        let start = lines.len();
        let task_l = task_lines(task, today, is_expanded(task), show_assignment, tag_styles);
        let task_count = task_l.len();
        lines.extend(task_l);
        // Each task ends with a blank separator we want to exclude from the
        // highlight band — only the content rows light up.
        ranges.push(start..start + task_count.saturating_sub(1));
    }
    (lines, ranges)
}

#[cfg(test)]
mod tests {
    use super::task_lines;
    use crate::tasks::task::test_task;
    use chrono::NaiveDate;
    use ratatui::text::Line;

    fn styles() -> crate::personalization::tags::TagStyles {
        crate::personalization::tags::TagStyles::with_overrides(&std::collections::BTreeMap::new())
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn flat(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect()
    }

    /// Count how many of `lines` contain the `◇` Linear glyph.
    fn linear_marker_rows(lines: &[Line<'_>]) -> usize {
        lines
            .iter()
            .filter(|l| l.spans.iter().any(|s| s.content.as_ref().contains('◇')))
            .count()
    }

    #[test]
    fn task_row_shows_linear_identifier_when_linked() {
        let today = d(2026, 6, 23);
        let mut t = test_task("T9", "not_started");
        t.linear_issue = "AVA-123".to_owned();
        let lines = task_lines(&t, today, false, false, &styles());
        let flat = flat(&lines);
        assert!(
            flat.contains("AVA-123"),
            "linked task row should surface the Linear identifier, got: {flat}"
        );
        // The full URL must not be dumped into the row (blows out width).
        assert!(
            !flat.contains("https://"),
            "row should show the identifier, not the long URL"
        );
    }

    #[test]
    fn expanded_task_row_does_not_duplicate_the_linear_marker() {
        let today = d(2026, 6, 23);
        let mut t = test_task("T9", "not_started");
        t.linear_issue = "AVA-123".to_owned();
        // Expanding notes must not add a second `◇` line (the full-URL line
        // that previously read as a duplicate of the header marker). The
        // header chip's compact marker is the only Linear row, expanded or not.
        let collapsed = task_lines(&t, today, false, false, &styles());
        let expanded = task_lines(&t, today, true, false, &styles());
        assert_eq!(linear_marker_rows(&collapsed), 1);
        assert_eq!(
            linear_marker_rows(&expanded),
            1,
            "expanding notes should not duplicate the Linear marker"
        );
        // And the verbose full URL is no longer dumped into the card.
        assert!(!flat(&expanded).contains("https://"));
    }

    #[test]
    fn task_row_has_no_linear_marker_when_unlinked() {
        let today = d(2026, 6, 23);
        let t = test_task("T9", "not_started"); // linear_issue empty
        assert!(!flat(&task_lines(&t, today, false, false, &styles())).contains("AVA"));
    }

    #[test]
    fn assignment_detail_is_visible_only_for_shared_workspace_mode() {
        let today = d(2026, 8, 3);
        let mut task = test_task("T9", "not_started");
        task.assigned_to = "wife".to_owned();

        let shared = flat(&task_lines(&task, today, false, true, &styles()));
        let personal = flat(&task_lines(&task, today, false, false, &styles()));

        assert!(shared.contains("assigned to wife"), "{shared}");
        assert!(!personal.contains("wife"), "{personal}");
    }
}
