//! Palette + styled line helpers for the picker UI. Tokyo-Night-inspired,
//! tuned to match the `tasks` aesthetic.

use std::collections::BTreeSet;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

pub const TEXT_PRIMARY: Color = Color::Rgb(192, 202, 245);
pub const TEXT_DIM: Color = Color::Rgb(122, 134, 173);
pub const TEXT_VERY_DIM: Color = Color::Rgb(78, 92, 122);

pub const ACCENT_PURPLE: Color = Color::Rgb(187, 154, 247);
pub const ACCENT_CYAN: Color = Color::Rgb(125, 207, 255);
/// Positive-action green (Tokyo-Night). Used for the "Create PDF"
/// confirmation modal — a constructive, non-destructive action.
pub const ACCENT_GREEN: Color = Color::Rgb(158, 206, 106);
/// Destructive-action red (Tokyo-Night). Used for the "Delete" confirmation
/// modal to warn that the action is irreversible-looking (it trashes a file).
pub const ACCENT_RED: Color = Color::Rgb(247, 118, 142);

const MATCH_HIGHLIGHT: Color = Color::Rgb(255, 199, 119);
pub const SELECTED_BG: Color = Color::Rgb(36, 40, 59);

// ---------------------------------------------------------------------------
// Reusable styles
// ---------------------------------------------------------------------------

const fn primary_bold() -> Style {
    Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD)
}

const fn dim() -> Style {
    Style::new().fg(TEXT_DIM)
}

const fn very_dim() -> Style {
    Style::new().fg(TEXT_VERY_DIM)
}

fn sep_span() -> Span<'static> {
    Span::styled(" · ", very_dim())
}

// ---------------------------------------------------------------------------
// Lines
// ---------------------------------------------------------------------------

#[must_use]
pub fn header_line(scope: &str, total: usize, matched: usize) -> Line<'static> {
    let title = Style::new().fg(ACCENT_PURPLE).add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::raw(" "),
        Span::styled("BRAIN", title),
        sep_span(),
        Span::styled(scope.to_owned(), primary_bold()),
        sep_span(),
        Span::styled(format!("{matched} of {total}"), dim()),
    ])
}

#[must_use]
pub fn input_line(query: &str) -> Line<'static> {
    let prompt = Style::new().fg(ACCENT_PURPLE).add_modifier(Modifier::BOLD);
    let text = Style::new().fg(TEXT_PRIMARY);
    // Block cursor: reversed space at the tail of the query.
    let cursor = Style::new().add_modifier(Modifier::REVERSED);
    Line::from(vec![
        Span::styled(" › ", prompt),
        Span::styled(query.to_owned(), text),
        Span::styled(" ", cursor),
    ])
}

#[must_use]
pub fn separator_line(width: usize) -> Line<'static> {
    Line::from(Span::styled("─".repeat(width), very_dim()))
}

/// One-line section heading shown above each bucket's matches.
/// Example: " Projects · 12"
#[must_use]
pub fn section_header_line(label: &str, count: usize) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(
            label.to_owned(),
            Style::new().fg(ACCENT_CYAN).add_modifier(Modifier::BOLD),
        ),
        sep_span(),
        Span::styled(format!("{count}"), very_dim()),
    ])
}

#[must_use]
pub fn empty_line(query_empty: bool) -> Line<'static> {
    let msg = if query_empty {
        "  No entries found in ~/brain."
    } else {
        "  No matches. Try a different query."
    };
    Line::from(Span::styled(
        msg,
        Style::new().fg(TEXT_DIM).add_modifier(Modifier::ITALIC),
    ))
}

/// Render one entry with fuzzy-match highlights.
///
/// `match_byte_positions` are byte offsets into `display` (nucleo-matcher
/// returns char positions over a `Utf32Str`; callers convert them to byte
/// offsets before passing in here).
#[must_use]
pub fn entry_line(
    display: &str,
    match_byte_positions: &BTreeSet<usize>,
    selected: bool,
) -> Line<'static> {
    let base = if selected {
        Style::new()
            .fg(TEXT_PRIMARY)
            .bg(SELECTED_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(TEXT_DIM)
    };
    let hl = if selected {
        Style::new()
            .fg(MATCH_HIGHLIGHT)
            .bg(SELECTED_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new()
            .fg(MATCH_HIGHLIGHT)
            .add_modifier(Modifier::BOLD)
    };

    let arrow_style = if selected {
        Style::new()
            .fg(ACCENT_CYAN)
            .bg(SELECTED_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(TEXT_VERY_DIM)
    };
    let arrow = if selected { " ❯ " } else { "   " };

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(display.len() + 1);
    spans.push(Span::styled(arrow.to_owned(), arrow_style));

    // Coalesce runs of same-style chars to keep the span count modest.
    let mut current_style = base;
    let mut current_text = String::new();
    for (byte_idx, ch) in display.char_indices() {
        let style = if match_byte_positions.contains(&byte_idx) {
            hl
        } else {
            base
        };
        if style != current_style && !current_text.is_empty() {
            spans.push(Span::styled(
                std::mem::take(&mut current_text),
                current_style,
            ));
        }
        current_style = style;
        current_text.push(ch);
    }
    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    Line::from(spans)
}

#[must_use]
pub fn footer_line() -> Line<'static> {
    let key = primary_bold();
    let lbl = dim();
    let dot = very_dim();
    Line::from(vec![
        Span::raw(" "),
        Span::styled("type", key),
        Span::styled(" filter", lbl),
        Span::styled("   ", dot),
        Span::styled("↑↓", key),
        Span::styled(" / ", dot),
        Span::styled("C-k C-n", key),
        Span::styled(" move", lbl),
        Span::styled("   ", dot),
        Span::styled("Enter", key),
        Span::styled(" open file", lbl),
        Span::styled("   ", dot),
        Span::styled("C-Enter", key),
        Span::styled(" reveal in Finder", lbl),
        Span::styled("   ", dot),
        Span::styled("C-p", key),
        Span::styled(" palette", lbl),
        Span::styled("   ", dot),
        Span::styled("Esc", key),
        Span::styled(" quit", lbl),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Concatenate a line's span contents back into a plain string.
    fn text_of(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn header_shows_scope_and_match_counts() {
        let line = header_line("search · 'rust'", 120, 7);
        let text = text_of(&line);
        assert!(text.contains("BRAIN"));
        assert!(text.contains("search · 'rust'"));
        assert!(text.contains("7 of 120"));
    }

    #[test]
    fn input_line_carries_query_and_a_cursor_cell() {
        let line = input_line("borrow");
        let text = text_of(&line);
        assert!(text.contains("borrow"));
        // A trailing reversed-space cursor cell is appended.
        let has_cursor = line
            .spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::REVERSED));
        assert!(has_cursor);
    }

    #[test]
    fn section_header_shows_label_and_count() {
        let text = text_of(&section_header_line("Projects", 12));
        assert!(text.contains("Projects"));
        assert!(text.contains("12"));
    }

    #[test]
    fn empty_line_differs_for_empty_vs_no_match() {
        assert!(text_of(&empty_line(true)).contains("No entries"));
        assert!(text_of(&empty_line(false)).contains("No matches"));
    }

    #[test]
    fn entry_line_preserves_the_full_display_text() {
        let display = "~/brain/projects/ann-afloat/plan.md";
        let line = entry_line(display, &BTreeSet::new(), false);
        let text = text_of(&line);
        assert!(text.ends_with(display), "got: {text}");
    }

    #[test]
    fn highlighted_bytes_get_the_match_color() {
        let display = "afloat";
        // Highlight the first two bytes ('a','f').
        let hl: BTreeSet<usize> = [0usize, 1].into_iter().collect();
        let line = entry_line(display, &hl, false);
        // Find a span whose text is the highlighted run and confirm its fg.
        let highlighted = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "af")
            .expect("the highlighted run should be coalesced into one span");
        assert_eq!(highlighted.style.fg, Some(MATCH_HIGHLIGHT));
    }

    #[test]
    fn selected_entry_uses_the_selection_background() {
        let line = entry_line("note.md", &BTreeSet::new(), true);
        assert!(
            line.spans
                .iter()
                .any(|s| s.style.bg == Some(SELECTED_BG)),
            "selected rows should paint the selection background"
        );
    }
}
