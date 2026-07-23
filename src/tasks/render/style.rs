//! The Tokyo-Night-ish palette plus the small style/label primitives and
//! inline span builders the rest of `render` composes from.

use chrono::NaiveDate;
use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};

// ---------------------------------------------------------------------------
// Palette (Tokyo-Night-ish; designed to read well on a dark terminal)
// ---------------------------------------------------------------------------

pub(super) const TEXT_PRIMARY: Color = Color::Rgb(192, 202, 245); // soft white
pub(super) const TEXT_DIM: Color = Color::Rgb(122, 134, 173); // medium dim
pub(super) const TEXT_VERY_DIM: Color = Color::Rgb(78, 92, 122); // very dim

pub(super) const ACCENT_PURPLE: Color = Color::Rgb(187, 154, 247); // IDs, brand mark
pub(super) const ACCENT_CYAN: Color = Color::Rgb(125, 207, 255); // project / link
pub(super) const ACCENT_GREEN: Color = Color::Rgb(158, 206, 106); // tomorrow / soon

const PRIO_P0: Color = Color::Rgb(247, 118, 142); // pink-red
const PRIO_P1: Color = Color::Rgb(255, 158, 100); // amber
const PRIO_P2: Color = Color::Rgb(224, 175, 104); // gold
const PRIO_P3: Color = Color::Rgb(125, 207, 255); // cool blue
const PRIO_P4: Color = Color::Rgb(78, 92, 122); // very dim

pub(super) const DUE_TODAY: Color = Color::Rgb(255, 199, 119); // gold
pub(super) const DUE_OVERDUE: Color = Color::Rgb(247, 118, 142); // pink-red
const DUE_FAR: Color = Color::Rgb(122, 134, 173); // dim

const STATUS_PROGRESS: Color = Color::Rgb(125, 207, 255); // cyan
pub(super) const STATUS_DONE: Color = Color::Rgb(70, 78, 110); // dim

const ACCENT_GLYPH: &str = "▎ ";

// ---------------------------------------------------------------------------
// Style + label primitives
// ---------------------------------------------------------------------------

#[must_use]
pub const fn priority_color(p: &str) -> Color {
    match p.as_bytes() {
        b"p0" => PRIO_P0,
        b"p1" => PRIO_P1,
        b"p2" => PRIO_P2,
        b"p3" => PRIO_P3,
        b"p4" => PRIO_P4,
        _ => TEXT_DIM,
    }
}

#[must_use]
pub const fn priority_style(p: &str) -> Style {
    Style::new()
        .fg(priority_color(p))
        .add_modifier(Modifier::BOLD)
}

#[must_use]
pub const fn status_style(s: &str) -> Style {
    match s.as_bytes() {
        b"done" => Style::new()
            .fg(STATUS_DONE)
            .add_modifier(Modifier::CROSSED_OUT),
        b"in_progress" => Style::new().fg(STATUS_PROGRESS),
        _ => Style::new().fg(TEXT_DIM),
    }
}

#[must_use]
pub const fn status_label(s: &str) -> &'static str {
    match s.as_bytes() {
        b"done" => "✓ done",
        b"in_progress" => "◐ in progress",
        b"not_started" => "○ not started",
        _ => "·",
    }
}

#[must_use]
pub const fn energy_icon(e: &str) -> &'static str {
    match e.as_bytes() {
        b"high" => "⚡",
        b"low" => "💤",
        _ => "·",
    }
}

/// Resolve a task type/tag to its display label via personalization.
///
/// Uses the generic defaults (`mit`/`personal`/`work`) plus the user's
/// overrides; unknown tags render as their raw name. The public binary carries
/// no personal taxonomy — see `personalization::tags`.
#[must_use]
pub fn type_label(t: &str) -> String {
    crate::personalization::tag_label(t)
}

#[must_use]
pub fn truncate(s: &str, max: usize) -> String {
    let flat: String = s.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
    if flat.chars().count() <= max {
        return flat;
    }
    let mut out: String = flat.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// Inline span builders
// ---------------------------------------------------------------------------

pub(super) fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(TEXT_DIM))
}

pub(super) fn very_dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(TEXT_VERY_DIM))
}

pub(super) fn sep() -> Span<'static> {
    Span::styled(" · ", Style::new().fg(TEXT_VERY_DIM))
}

pub(super) fn accent(priority: &str) -> Span<'static> {
    Span::styled(
        ACCENT_GLYPH,
        Style::new()
            .fg(priority_color(priority))
            .add_modifier(Modifier::BOLD),
    )
}

#[must_use]
pub fn due_span(due: Option<NaiveDate>, today: NaiveDate, done: bool) -> Span<'static> {
    let Some(d) = due else {
        return Span::styled("no due date", Style::new().fg(TEXT_VERY_DIM));
    };
    if done {
        return Span::styled(
            d.to_string(),
            Style::new()
                .fg(STATUS_DONE)
                .add_modifier(Modifier::CROSSED_OUT),
        );
    }
    let diff = (d - today).num_days();
    let (text, color, bold) = match diff {
        i64::MIN..=-1 => (format!("overdue {}d ({d})", -diff), DUE_OVERDUE, true),
        0 => (format!("today ({d})"), DUE_TODAY, true),
        1 => (format!("tomorrow ({d})"), ACCENT_GREEN, true),
        2..=7 => (format!("in {diff}d ({d})"), ACCENT_GREEN, false),
        8..=30 => (format!("in {diff}d ({d})"), ACCENT_CYAN, false),
        _ => (d.to_string(), DUE_FAR, false),
    };
    let mut style = Style::new().fg(color);
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    Span::styled(text, style)
}

#[cfg(test)]
mod tests {
    use super::{due_span, truncate, type_label};
    use chrono::NaiveDate;
    use ratatui::style::Modifier;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    // --- truncate ---

    #[test]
    fn truncate_passes_short_strings_through() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_appends_ellipsis_when_over_max() {
        // max=4: chars 0..3 ("abc") + "…" = "abc…"
        assert_eq!(truncate("abcdef", 4), "abc…");
    }

    #[test]
    fn truncate_replaces_newlines_with_spaces_before_counting() {
        assert_eq!(truncate("a\nb", 5), "a b");
    }

    #[test]
    fn truncate_handles_multibyte_chars_without_panicking() {
        // 5 emojis is 5 chars (chrono::chars), 20 bytes. max=3 → 2 + ellipsis.
        let out = truncate("🔥🔥🔥🔥🔥", 3);
        assert_eq!(out.chars().count(), 3);
        assert!(out.ends_with('…'));
    }

    // --- due_span ---

    #[test]
    fn due_span_no_date_is_dimmed_placeholder() {
        let today = d(2026, 6, 23);
        let span = due_span(None, today, false);
        assert!(span.content.contains("no due date"));
    }

    #[test]
    fn due_span_today_says_today_and_is_bold() {
        let today = d(2026, 6, 23);
        let span = due_span(Some(today), today, false);
        assert!(span.content.contains("today"));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn due_span_tomorrow_uses_tomorrow_label() {
        let today = d(2026, 6, 23);
        let span = due_span(Some(d(2026, 6, 24)), today, false);
        assert!(span.content.contains("tomorrow"));
    }

    #[test]
    fn due_span_within_week_uses_in_nd_label() {
        let today = d(2026, 6, 23);
        let span = due_span(Some(d(2026, 6, 28)), today, false);
        assert!(span.content.contains("in 5d"));
    }

    #[test]
    fn due_span_overdue_shows_days_overdue() {
        let today = d(2026, 6, 23);
        let span = due_span(Some(d(2026, 6, 20)), today, false);
        assert!(span.content.contains("overdue 3d"));
    }

    #[test]
    fn due_span_done_overrides_to_crossed_out_date_only() {
        let today = d(2026, 6, 23);
        let span = due_span(Some(d(2026, 6, 20)), today, true);
        // No "overdue Nd" prefix for done tasks — just the date.
        assert!(!span.content.contains("overdue"));
        assert!(span.style.add_modifier.contains(Modifier::CROSSED_OUT));
    }

    // --- type_label (generic defaults; no personalization loaded in tests) ---

    #[test]
    fn type_label_generic_default_emits_emoji_label() {
        // `mit` ships as a universal default.
        assert_eq!(type_label("mit"), "❗ MIT");
    }

    #[test]
    fn type_label_non_default_tag_falls_back_to_raw_name() {
        // `code` is no longer a built-in — it lives in a user's personalization
        // now, so with none loaded it renders as its raw name.
        assert_eq!(type_label("code"), "code");
        assert_eq!(type_label("custom"), "custom");
    }
}
