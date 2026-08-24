//! Re-deriving the two snapshot sections — "Today's habits" and "Completed
//! today" — from the CSVs.
//!
//! These are rebuilt on **every** sync, not patched line by line, so a habit
//! flipped to done outside this process (another agent session, `/triage`, a
//! hand edit) still shows up correctly.

use chrono::NaiveDate;

use super::doc::Section;
use crate::tasks::complete::{Row, field};

/// Two-column markdown table body, blank-line padded so the surrounding
/// headings keep their spacing.
fn table(cells: &[String]) -> Vec<String> {
    let mut padded = cells.to_vec();
    if padded.len() % 2 != 0 {
        padded.push(String::new());
    }
    let mut body = vec![String::new(), "|  |  |".to_owned(), "|---|---|".to_owned()];
    for pair in padded.chunks(2) {
        body.push(format!("| {} | {} |", pair[0], pair[1]));
    }
    body.push(String::new());
    body
}

/// Sort habits by ideal time (unset last), then duration, then name.
fn sort_key(row: &Row) -> ((u8, String), i64, String) {
    let ideal = field(row, "ideal_time").trim().to_owned();
    let ideal_key = if ideal.is_empty() {
        (1, String::new())
    } else {
        (0, ideal)
    };
    let duration = field(row, "estimated_duration")
        .trim()
        .parse::<i64>()
        .unwrap_or(i64::MAX);
    (ideal_key, duration, field(row, "task_name").to_lowercase())
}

fn label(row: &Row, marker: char) -> String {
    format!(
        "{marker} **{}** {}",
        field(row, "task_id"),
        field(row, "task_name")
    )
}

fn is_done(row: &Row) -> bool {
    field(row, "status").trim() == "done"
}

fn completed_today(row: &Row, today: NaiveDate) -> bool {
    field(row, "completed_date").trim() == today.to_string()
}

/// The "🔁 Today's habits" section: everything still pending today (no due date
/// or due on/before today) followed by today's completions. `None` when no
/// habit qualifies, so the section is dropped rather than left empty.
pub(super) fn today_habits(habits: &[Row], today: NaiveDate) -> Option<Section> {
    let mut pending: Vec<&Row> = Vec::new();
    let mut done: Vec<&Row> = Vec::new();
    for row in habits {
        let due = field(row, "due_date").trim().to_owned();
        if is_done(row) {
            if completed_today(row, today) {
                done.push(row);
            }
        } else if due.is_empty() || due <= today.to_string() {
            pending.push(row);
        }
    }
    pending.sort_by_key(|row| sort_key(row));
    done.sort_by_key(|row| sort_key(row));

    let cells: Vec<String> = pending
        .iter()
        .map(|row| label(row, '◻'))
        .chain(done.iter().map(|row| label(row, '✅')))
        .collect();
    if cells.is_empty() {
        return None;
    }
    Some(Section {
        heading: "## 🔁 Today's habits".to_owned(),
        body: table(&cells),
    })
}

/// The "✅ Completed today" section, habits first then tasks (the order the
/// agenda has always listed them in). `None` when nothing was completed today.
pub(super) fn completed_today_section(
    tasks: &[Row],
    habits: &[Row],
    today: NaiveDate,
) -> Option<Section> {
    let cells: Vec<String> = habits
        .iter()
        .chain(tasks.iter())
        .filter(|row| is_done(row) && completed_today(row, today))
        .map(|row| label(row, '✅'))
        .collect();
    if cells.is_empty() {
        return None;
    }
    Some(Section {
        heading: "## ✅ Completed today".to_owned(),
        body: table(&cells),
    })
}
