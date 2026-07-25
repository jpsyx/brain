//! Pure HTML rendering for the habits page.
//!
//! Ports the Python `render_card` / `render_section` / `render_page` family to
//! Rust: cards are grouped by time-of-day section, then by priority
//! sub-section, then injected into the `web/habits/` shell along with the
//! inlined CSS and JS. Every function here is pure — no clock, no IO — so the
//! output is fully unit-testable.

use std::fmt::Write as _;

use chrono::{Datelike, NaiveDate};

use super::model::{Habit, TimeBucket, PRIORITY_ORDER};

/// The page shell and its two inlined assets, embedded at compile time.
const SHELL: &str = include_str!("../../../../web/habits/index.html");
const CSS: &str = include_str!("../../../../web/habits/style.css");
const APP_JS: &str = include_str!("../../../../web/habits/app.js");

/// Accent color for a priority's bar/dot (falls back to a neutral gray).
fn priority_color(priority: &str) -> &'static str {
    match priority {
        "p0" => "#cf5b52",
        "p1" => "#d39148",
        "p2" => "#5f82b3",
        "p3" => "#8a94a2",
        "p4" => "#aeb4be",
        _ => "#9ca3af",
    }
}

/// Human label for a priority sub-section header.
fn priority_label(priority: &str) -> &'static str {
    match priority {
        "p0" => "P0 · urgent",
        "p1" => "P1 · high",
        "p2" => "P2 · medium",
        "p3" => "P3 · low",
        _ => "P4 · someday",
    }
}

/// HTML-escape text for both element content and quoted attributes (mirrors
/// Python's `html.escape(..., quote=True)`).
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// One pending-habit card.
fn render_card(h: &Habit, today: NaiveDate) -> String {
    let name = if h.name.is_empty() { "(unnamed)" } else { &h.name };
    let color = priority_color(&h.priority);

    let mut badges = String::new();
    if let Some(ideal) = h.ideal_time.as_deref() {
        let _ = write!(
            badges,
            r#"<span class="meta-chip meta-time">{}</span>"#,
            escape(ideal)
        );
    }
    if let Some(dur) = h.estimated_duration {
        let _ = write!(badges, r#"<span class="meta-chip">{dur}m</span>"#);
    }
    if let Some(due) = h.due_date {
        if due < today {
            let delta = (today - due).num_days();
            let _ = write!(
                badges,
                r#"<span class="meta-chip meta-pastdue">{delta}d past-due</span>"#
            );
        } else if due == today {
            badges.push_str(r#"<span class="meta-chip meta-today">today</span>"#);
        }
    }
    if h.hard_deadline {
        badges.push_str(r#"<span class="meta-chip meta-hard">hard</span>"#);
    }

    let title_attr = escape(&h.notes);
    let esc_id = escape(&h.task_id);
    let esc_name = escape(name);
    format!(
        r#"<article class="card" data-task-id="{esc_id}" title="{title_attr}">
  <div class="pri-bar" style="background:{color};"></div>
  <div class="card-body">
    <div class="card-title">{esc_name}</div>
    <div class="meta-row">{badges}</div>
  </div>
  <button class="done-btn" type="button"
          data-task-id="{esc_id}"
          aria-label="Mark {esc_name} done">✓</button>
</article>"#
    )
}

/// One completed-habit card (no done button, muted styling).
fn render_completed_card(h: &Habit) -> String {
    let name = if h.name.is_empty() { "(unnamed)" } else { &h.name };
    let mut chips = String::new();
    if let Some(dur) = h.estimated_duration {
        let _ = write!(chips, r#"<span class="meta-chip">{dur}m</span>"#);
    }
    chips.push_str(r#"<span class="meta-chip meta-done">✓ done</span>"#);
    format!(
        r#"<article class="card card--completed" title="{}">
  <div class="pri-bar pri-bar--muted"></div>
  <div class="card-body">
    <div class="card-title">{}</div>
    <div class="meta-row">{chips}</div>
  </div>
</article>"#,
        escape(&h.notes),
        escape(name),
    )
}

/// A priority sub-section (dot + label + count + a grid of its cards).
fn render_priority_subsection(priority: &str, rows: &[&Habit], today: NaiveDate) -> String {
    let cards = rows
        .iter()
        .map(|h| render_card(h, today))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<div class="time-section" data-priority="{priority}">
  <header class="time-header">
    <span class="pri-dot" style="background:{color};"></span>
    <span class="time-label">{label}</span>
    <span class="time-count">{count}</span>
  </header>
  <div class="grid">
    {cards}
  </div>
</div>"#,
        color = priority_color(priority),
        label = priority_label(priority),
        count = rows.len(),
    )
}

/// Render the full habits page as a self-contained HTML document.
#[must_use]
pub fn render(pending: &[Habit], completed: &[Habit], today: NaiveDate) -> String {
    let body = if pending.is_empty() {
        r#"<div class="empty">All habits done for today. Nice work.</div>"#.to_owned()
    } else {
        TimeBucket::ALL
            .iter()
            .filter_map(|&bucket| {
                let rows: Vec<&Habit> =
                    pending.iter().filter(|h| h.bucket() == bucket).collect();
                (!rows.is_empty()).then(|| render_time_section(bucket, &rows, today))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let completed_cards = completed
        .iter()
        .map(render_completed_card)
        .collect::<Vec<_>>()
        .join("\n");
    let completed_display = if completed.is_empty() { "none" } else { "" };

    let today_label = format!(
        "{}{}{}",
        today.format("%A, %B "),
        today.day(),
        today.format(", %Y")
    );

    SHELL
        .replace("{{CSS}}", CSS)
        .replace("{{JS}}", APP_JS)
        .replace("{{TODAY_LABEL}}", &escape(&today_label))
        .replace("{{COUNT}}", &pending.len().to_string())
        .replace("{{COMPLETED_DISPLAY}}", completed_display)
        .replace("{{COMPLETED_COUNT}}", &completed.len().to_string())
        .replace("{{COMPLETED_CARDS}}", &completed_cards)
        .replace("{{BODY}}", &body)
}

/// A time-of-day section: its header, then each non-empty priority sub-section
/// (priorities in [`PRIORITY_ORDER`]).
fn render_time_section(bucket: TimeBucket, rows: &[&Habit], today: NaiveDate) -> String {
    let subs = PRIORITY_ORDER
        .iter()
        .filter_map(|&priority| {
            let pri_rows: Vec<&Habit> =
                rows.iter().copied().filter(|h| h.priority == priority).collect();
            (!pri_rows.is_empty())
                .then(|| render_priority_subsection(priority, &pri_rows, today))
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<section class="pri-section" data-bucket="{bucket}">
  <header class="pri-header">
    <span class="pri-label">{label}</span>
    <span class="pri-count">{count}</span>
  </header>
  {subs}
</section>"#,
        bucket = bucket_slug(bucket),
        label = bucket.label(),
        count = rows.len(),
    )
}

/// Lowercase data attribute slug for a bucket (`morning`, `afternoon`, …).
fn bucket_slug(bucket: TimeBucket) -> &'static str {
    match bucket {
        TimeBucket::Morning => "morning",
        TimeBucket::Afternoon => "afternoon",
        TimeBucket::Evening => "evening",
        TimeBucket::Anytime => "anytime",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::routes::habits::model::Habit;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn habit(id: &str, name: &str) -> Habit {
        Habit {
            task_id: id.to_owned(),
            name: name.to_owned(),
            status: "not_started".to_owned(),
            priority: "p1".to_owned(),
            due_date: None,
            hard_deadline: false,
            estimated_duration: Some(10),
            ideal_time: Some("9:00 AM".to_owned()),
            notes: String::new(),
            completed_date: None,
        }
    }

    #[test]
    fn render_includes_pending_habit_name_and_id() {
        let today = day(2026, 7, 25);
        let html = render(&[habit("H7", "Floss teeth")], &[], today);
        assert!(html.contains("Floss teeth"), "habit name must render");
        assert!(
            html.contains(r#"data-task-id="H7""#),
            "card must carry its task id"
        );
    }

    #[test]
    fn render_injects_the_shell_css_and_js() {
        let today = day(2026, 7, 25);
        let html = render(&[habit("H1", "x")], &[], today);
        assert!(html.contains(".done-btn"), "CSS must be inlined");
        assert!(
            html.contains("/habits/done"),
            "JS must post to the brain-server endpoint"
        );
        assert!(!html.contains("{{CSS}}"), "no shell token may leak");
        assert!(!html.contains("{{BODY}}"), "no shell token may leak");
    }

    #[test]
    fn render_escapes_html_in_names() {
        let today = day(2026, 7, 25);
        let html = render(&[habit("H1", "<b>hack</b>")], &[], today);
        assert!(html.contains("&lt;b&gt;hack&lt;/b&gt;"), "name must be escaped");
        assert!(!html.contains("<b>hack</b>"), "raw markup must not survive");
    }

    #[test]
    fn render_empty_state_when_no_pending() {
        let today = day(2026, 7, 25);
        let html = render(&[], &[], today);
        assert!(html.contains("All habits done for today"));
    }

    #[test]
    fn render_shows_completed_card() {
        let today = day(2026, 7, 25);
        let mut done = habit("H9", "Meditate");
        done.status = "done".to_owned();
        done.completed_date = Some(today);
        let html = render(&[], &[done], today);
        assert!(html.contains("Meditate"));
        assert!(html.contains("✓ done"));
    }

    #[test]
    fn render_past_due_badge_counts_days() {
        let today = day(2026, 7, 25);
        let mut h = habit("H1", "Overdue thing");
        h.due_date = Some(day(2026, 7, 22));
        let html = render(&[h], &[], today);
        assert!(html.contains("3d past-due"), "overdue delta must render");
    }
}
