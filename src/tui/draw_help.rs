//! Drawing the keyboard-shortcuts help modal (`?`). Renders every binding in
//! `shortcuts::ALL`, grouped by section, with a key column and a description.
//! It's a captive centered overlay; the body scrolls when it's taller than
//! the modal.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::tasks::shortcuts::{self, Group};
use crate::tui::draw::layout::centered_rect;
use crate::tui::modal_state::HelpState;

/// The narrowest the help modal ever gets on a terminal with room to spare.
/// Below this the descriptions wrap so hard the two-column layout stops
/// reading as a table.
const HELP_MIN_WIDTH: u16 = 70;

/// The widest we let it grow. Past this a one-line description drifts so far
/// from its key column that the eye loses the row.
const HELP_MAX_WIDTH: u16 = 112;

/// The help modal's width for a terminal `available` columns wide.
///
/// The old fixed 70 columns wrapped nearly every description onto a second
/// line. This keeps a ~10% side gutter so the modal still reads as an overlay,
/// clamps to a legible band, and falls back to the whole terminal when even
/// the minimum doesn't fit.
pub(crate) fn help_modal_width(available: u16) -> u16 {
    let gutter = available / 10;
    available
        .saturating_sub(gutter)
        .clamp(HELP_MIN_WIDTH, HELP_MAX_WIDTH)
        .min(available)
}

const ACCENT: Color = Color::Rgb(187, 154, 247);
const KEY: Color = Color::Rgb(255, 199, 119);
const HEADING: Color = Color::Rgb(125, 207, 255);
const TEXT: Color = Color::Rgb(192, 202, 245);
const DIM: Color = Color::Rgb(122, 134, 173);

/// Build the full body of help lines, grouped by [`Group::ORDER`]. The key
/// column is left-padded to a fixed width so the descriptions align.
pub(crate) fn help_lines() -> Vec<Line<'static>> {
    // Widest key string across all rows, so the description column lines up.
    let key_w = shortcuts::ALL
        .iter()
        .map(|s| s.keys.chars().count())
        .max()
        .unwrap_or(0);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (gi, group) in Group::ORDER.iter().enumerate() {
        let rows = shortcuts::in_group(*group);
        if rows.is_empty() {
            continue;
        }
        if gi > 0 {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            format!(" {}", group.title()),
            Style::default().fg(HEADING).add_modifier(Modifier::BOLD),
        )));
        for s in rows {
            let pad = key_w.saturating_sub(s.keys.chars().count());
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::raw(" ".repeat(pad)),
                Span::styled(
                    s.keys,
                    Style::default().fg(KEY).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  ", Style::default().fg(DIM)),
                Span::styled(s.desc, Style::default().fg(TEXT)),
            ]));
        }
    }
    lines
}

pub(crate) fn draw_help(f: &mut Frame, state: &HelpState, area: Rect) {
    let lines = help_lines();
    let body_h = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    // +2 border, +1 footer.
    let want_h = body_h.saturating_add(3);
    let height = want_h.min(area.height.saturating_sub(2)).max(6);
    let width = help_modal_width(area.width);
    let modal = centered_rect(width, height, area);
    f.render_widget(Clear, modal);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Keyboard shortcuts",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    // Clamp scroll so we never run off the end of the list.
    let view_h = chunks[0].height;
    let max_scroll = body_h.saturating_sub(view_h);
    let scroll = state.scroll.min(max_scroll);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        chunks[0],
    );

    let key = Style::default().fg(TEXT).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(DIM);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled("↑↓", key),
            Span::styled(" scroll  ", dim),
            Span::styled("? / Esc / q", key),
            Span::styled(" close", dim),
        ])),
        chunks[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roomy_terminal_gets_a_modal_wider_than_the_old_fixed_70() {
        // The modal used to be pinned at 70 columns, which wrapped almost
        // every description onto a second line. On a wide terminal it should
        // use the room it has.
        assert!(help_modal_width(200) > 70, "got {}", help_modal_width(200));
        assert!(help_modal_width(120) > 70, "got {}", help_modal_width(120));
    }

    #[test]
    fn the_modal_never_outgrows_the_terminal_and_keeps_a_gutter() {
        for available in [20_u16, 40, 60, 70, 80, 120, 200, 400] {
            let width = help_modal_width(available);
            assert!(
                width <= available,
                "width {width} overflows available {available}"
            );
        }
        // Wide terminals keep a side gutter rather than filling edge to edge.
        assert!(help_modal_width(200) < 200);
    }

    #[test]
    fn a_narrow_terminal_still_uses_everything_it_has() {
        assert_eq!(help_modal_width(50), 50);
    }

    #[test]
    fn help_lines_include_a_heading_and_a_binding() {
        let lines = help_lines();
        assert!(!lines.is_empty());
        // The rendered text contains at least the Navigation heading and the
        // brain-close key somewhere.
        let flat: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.clone()))
            .collect();
        assert!(flat.contains("Navigation"));
        assert!(flat.contains("Brain directory"));
        assert!(flat.contains("^X"));
        assert!(flat.contains("every command"));
    }
}
