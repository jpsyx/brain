//! Rendering the palette as a centered modal overlay. The host draws whatever
//! it wants first; `draw_modal` clears its box region and paints the input,
//! separator, filtered rows, and footer on top. All the sizing/line builders
//! (`palette_width`, `number_prefix`, `item_line`) are pure.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::render;

use super::SearchPalette;
use super::model::SearchAction;
use crate::tui::PaletteRow;

#[cfg(test)]
use super::model::shortcut_for;

/// Columns the palette modal needs so its widest row renders — label **and**
/// shortcut hint — without being clipped by the border. Mirrors the fixed
/// decorations in [`item_line`]: 2 leading spaces, the right-aligned number
/// prefix (digits + `.`), the 3-column arrow gutter, the label, then the
/// `  [key]` hint. Floored so the `type filter …` footer always fits, and
/// padded on the right so long rows don't butt against the border.
fn palette_width(rows: &[PaletteRow<SearchAction>]) -> usize {
    const LEAD: usize = 2; // the two leading spaces
    const ARROW: usize = 3; // " ❯ " / "   "
    const HINT_FRAME: usize = 4; // "  [" + "]"
    const BORDERS: usize = 2;
    const RIGHT_PAD: usize = 2;
    const FOOTER_MIN: usize = 42; // the footer's " type filter … Esc back" width
    let num = rows.len().to_string().len() + 1; // right-aligned digits + '.'
    let content = rows
        .iter()
        .map(|row| {
            let hint = row
                .shortcut
                .map_or(0, |key| HINT_FRAME + key.chars().count());
            LEAD + num + ARROW + row.label.chars().count() + hint
        })
        .max()
        .unwrap_or(0)
        .max(FOOTER_MIN);
    content + BORDERS + RIGHT_PAD
}

/// Render the command palette as a centered modal overlay.
///
/// Drawn on top of whatever the host already rendered; `Clear` wipes the box
/// region first so the content behind doesn't bleed through.
pub(crate) fn draw_modal(f: &mut Frame, app: &SearchPalette, area: Rect) {
    // Tall enough for every row plus chrome (border 2 + input + separator +
    // footer = 5), clamped to the screen.
    let rows = u16::try_from(app.filtered().len().max(1)).unwrap_or(u16::MAX);
    let height = rows.saturating_add(5).min(area.height);
    let width = u16::try_from(palette_width(app.rows()))
        .unwrap_or(u16::MAX)
        .min(area.width);
    let modal = centered_rect(width, height, area);

    f.render_widget(Clear, modal);
    let accent = Style::new()
        .fg(render::ACCENT_PURPLE)
        .add_modifier(Modifier::BOLD);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(render::ACCENT_PURPLE))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled("Command palette", accent),
            Span::raw(" "),
        ]));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // input
            Constraint::Length(1), // separator
            Constraint::Min(1),    // list
            Constraint::Length(1), // footer
        ])
        .split(inner);

    f.render_widget(Paragraph::new(render::input_line(app.query())), chunks[0]);
    f.render_widget(
        Paragraph::new(render::separator_line(inner.width as usize)),
        chunks[1],
    );

    if app.filtered().is_empty() {
        f.render_widget(Paragraph::new(render::empty_line(false)), chunks[2]);
    } else {
        let item_lines: Vec<Line<'static>> = app
            .filtered()
            .iter()
            .enumerate()
            .map(|(list_index, &item_idx)| {
                let row = &app.rows()[item_idx];
                item_line(
                    item_idx,
                    app.rows().len(),
                    &row.label,
                    list_index == app.selected(),
                    row.shortcut,
                )
            })
            .collect();
        f.render_widget(Paragraph::new(item_lines), chunks[2]);
    }

    let key = Style::new()
        .fg(render::TEXT_PRIMARY)
        .add_modifier(Modifier::BOLD);
    let lbl = Style::new().fg(render::TEXT_DIM);
    let dot = Style::new().fg(render::TEXT_VERY_DIM);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled("type", key),
            Span::styled(" filter", lbl),
            Span::styled("  ", dot),
            Span::styled("↑↓", key),
            Span::styled(" move", lbl),
            Span::styled("  ", dot),
            Span::styled("Enter", key),
            Span::styled(" run", lbl),
            Span::styled("  ", dot),
            Span::styled("Esc", key),
            Span::styled(" back", lbl),
        ])),
        chunks[3],
    );
}

/// A `width`×`height` rectangle centered within `area` (clamped to it).
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// The `"N."` prefix for a row, right-aligned to the width of the largest
/// number in a `total`-row palette. Once the palette hits double digits, the
/// single-digit rows gain a leading space so every dot lines up.
fn number_prefix(index: usize, total: usize) -> String {
    let width = total.to_string().len();
    format!("{:>width$}.", index + 1)
}

fn item_line(
    index: usize,
    total: usize,
    label: &str,
    selected: bool,
    shortcut: Option<&'static str>,
) -> Line<'static> {
    let arrow = if selected { " ❯ " } else { "   " };
    let (num_style, arrow_style, label_style) = if selected {
        (
            Style::new()
                .fg(render::ACCENT_CYAN)
                .bg(render::SELECTED_BG)
                .add_modifier(Modifier::BOLD),
            Style::new()
                .fg(render::ACCENT_CYAN)
                .bg(render::SELECTED_BG)
                .add_modifier(Modifier::BOLD),
            Style::new()
                .fg(render::TEXT_PRIMARY)
                .bg(render::SELECTED_BG)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::new().fg(render::TEXT_VERY_DIM),
            Style::new().fg(render::TEXT_VERY_DIM),
            Style::new().fg(render::TEXT_DIM),
        )
    };
    let mut spans = vec![
        Span::styled("  ", num_style),
        Span::styled(number_prefix(index, total), num_style),
        Span::styled(arrow.to_owned(), arrow_style),
        Span::styled(label.to_owned(), label_style),
    ];
    // The shortcut hint stays dim regardless of selection — it's metadata,
    // not part of the focused-row emphasis.
    if let Some(key) = shortcut {
        spans.push(Span::styled(
            format!("  [{key}]"),
            Style::new().fg(render::TEXT_VERY_DIM),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::super::labels::open_dir_label;
    use super::*;

    #[test]
    fn palette_width_fits_the_widest_row_including_its_shortcut() {
        // A contextual "Open directory" row elided to the label budget, plus
        // its `[^↵]` hint, is about the widest thing the palette draws. The
        // modal must be wide enough to show that hint without clipping it at
        // the border (the bug in the screenshot).
        let rows = vec![
            PaletteRow::new(
                open_dir_label("projects/personal__foo/docs/integrations"),
                SearchAction::OpenDir,
                shortcut_for(SearchAction::OpenDir),
            ),
            PaletteRow::new(
                "Search projects",
                SearchAction::SearchProjects,
                shortcut_for(SearchAction::SearchProjects),
            ),
        ];
        let width = palette_width(&rows);

        // Reconstruct the widest row's rendered column count: 2 leading spaces
        // + right-aligned number prefix + 3-col arrow gutter + label + hint.
        let num = rows.len().to_string().len() + 1;
        let first = &rows[0];
        let hint = 4 + first.shortcut.unwrap().chars().count();
        let row = 2 + num + 3 + first.label.chars().count() + hint;

        assert!(
            width >= row + 2, // + the two side borders
            "modal width {width} must fit row {row} plus its borders"
        );
    }

    #[test]
    fn single_digit_palette_has_no_padding() {
        // 9 or fewer rows: every number is one digit, so no leading space.
        assert_eq!(number_prefix(0, 9), "1.");
        assert_eq!(number_prefix(8, 9), "9.");
    }

    #[test]
    fn double_digit_palette_pads_single_digit_numbers() {
        // 10+ rows: single-digit numbers gain a leading space so the dots
        // (and everything after them) line up with the two-digit rows.
        assert_eq!(number_prefix(0, 12), " 1.");
        assert_eq!(number_prefix(8, 12), " 9.");
        assert_eq!(number_prefix(9, 12), "10.");
        assert_eq!(number_prefix(11, 12), "12.");
    }

    #[test]
    fn shortcut_hint_is_rendered_dim_next_to_its_row() {
        // Row 0 is "Message brain" → carries the ^M hint.
        let line = item_line(
            0,
            9,
            "Message brain",
            false,
            shortcut_for(SearchAction::Global(crate::tui::GlobalAction::MessageBrain)),
        );
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("[^M]"), "got: {text}");
        let hint = line
            .spans
            .iter()
            .find(|s| s.content.contains("^M"))
            .expect("the ^M hint span exists");
        assert_eq!(hint.style.fg, Some(render::TEXT_VERY_DIM));
    }
}
