//! Drawing the command palette, the task actions modal, and the two target
//! pickers — all four are the same filterable list, so they share one renderer.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::tui::draw::layout::centered_rect;
use crate::tui::palette::{CommandPaletteState, EntryTargetPicker, TaskTargetPicker};

const ACCENT: Color = Color::Rgb(187, 154, 247);
const TEXT: Color = Color::Rgb(192, 202, 245);
const DIM: Color = Color::Rgb(122, 134, 173);
const SEPARATOR: Color = Color::Rgb(78, 92, 122);
const ACTIVE: Color = Color::Rgb(255, 199, 119);

/// Fixed columns each row spends before its label: the 3-column selection
/// gutter.
const GUTTER: u16 = 3;
/// `"  ["` + `"]"` around a shortcut hint.
const HINT_FRAME: u16 = 4;
/// Two side borders plus a right-hand breathing gap.
const CHROME: u16 = 4;
/// The footer hint's own width, so the modal never clips it.
const FOOTER_MIN: u16 = 46;
/// Border (2) + filter + separator + footer.
const VERTICAL_CHROME: u16 = 5;

/// One list-shaped modal, ready to draw.
pub(crate) struct PaletteView<'a> {
    pub(crate) title: &'a str,
    pub(crate) subtitle: Option<&'a str>,
    pub(crate) query: &'a str,
    pub(crate) entries: &'a [(String, Option<&'static str>)],
    pub(crate) selected: usize,
}

/// The columns the modal needs so its widest row renders — label *and*
/// shortcut hint — without being clipped, floored so the footer still fits and
/// capped at what the terminal has.
pub(crate) fn palette_width(entries: &[(String, Option<&'static str>)], available: u16) -> u16 {
    let widest = entries
        .iter()
        .map(|(label, shortcut)| {
            let hint = shortcut.map_or(0, |key| HINT_FRAME + text_width(key));
            GUTTER + text_width(label) + hint
        })
        .max()
        .unwrap_or(0);
    widest
        .saturating_add(CHROME)
        .max(FOOTER_MIN)
        .min(available)
}

/// The first row to render so the selection stays on screen. The list scrolls
/// only as far as it must: the selection sits still until it reaches an edge.
pub(crate) const fn viewport_start(selected: usize, total: usize, height: usize) -> usize {
    if height == 0 || total <= height {
        return 0;
    }
    let last_start = total - height;
    if selected < height {
        return 0;
    }
    let start = selected + 1 - height;
    if start > last_start { last_start } else { start }
}

fn text_width(value: &str) -> u16 {
    u16::try_from(value.chars().count()).unwrap_or(u16::MAX)
}

pub(crate) fn draw_palette(f: &mut Frame, state: &CommandPaletteState, area: Rect) {
    let entries = state.numbered_entries();
    let subtitle = state.task_actions_modal().then(|| state.subtitle()).flatten();
    draw_palette_view(
        f,
        &PaletteView {
            title: state.title(),
            subtitle,
            query: state.query(),
            entries: &entries,
            selected: state.selected(),
        },
        area,
    );
}

pub(crate) fn draw_task_target_picker(f: &mut Frame, state: &TaskTargetPicker, area: Rect) {
    let entries = state.numbered_entries();
    draw_palette_view(
        f,
        &PaletteView {
            title: state.title(),
            subtitle: Some("choose the task this command runs on"),
            query: state.query(),
            entries: &entries,
            selected: state.selected(),
        },
        area,
    );
}

/// The entry picker is the brain-directory fuzzy picker itself, boxed as a
/// modal so it looks and behaves the same wherever a command raises it.
pub(crate) fn draw_entry_target_picker(f: &mut Frame, state: &mut EntryTargetPicker, area: Rect) {
    let width = area.width.saturating_sub(area.width / 6).max(40).min(area.width);
    let height = area.height.saturating_sub(4).max(8).min(area.height);
    let modal = centered_rect(width, height, area);
    f.render_widget(Clear, modal);

    let block = bordered(state.title());
    let inner = block.inner(modal);
    f.render_widget(block, modal);
    crate::picker::draw_into(f, state.picker_mut(), inner);
}

pub(crate) fn draw_palette_view(f: &mut Frame, view: &PaletteView, area: Rect) {
    let subtitle_rows = u16::from(view.subtitle.is_some());
    let rows = u16::try_from(view.entries.len().max(1)).unwrap_or(u16::MAX);
    let height = rows
        .saturating_add(VERTICAL_CHROME)
        .saturating_add(subtitle_rows)
        .min(area.height);
    let modal = centered_rect(palette_width(view.entries, area.width), height, area);
    f.render_widget(Clear, modal);

    let block = bordered(view.title);
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let mut constraints: Vec<Constraint> = Vec::with_capacity(5);
    if subtitle_rows == 1 {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(1)); // filter
    constraints.push(Constraint::Length(1)); // separator
    constraints.push(Constraint::Min(1)); // list
    constraints.push(Constraint::Length(1)); // footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let mut index = 0_usize;
    if let Some(subtitle) = view.subtitle {
        let max_chars = usize::from(inner.width).saturating_sub(2).max(8);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    crate::tasks::render::truncate(subtitle, max_chars),
                    Style::default().fg(DIM),
                ),
            ])),
            chunks[index],
        );
        index += 1;
    }

    draw_filter(f, view.query, chunks[index]);
    index += 1;

    let separator = chunks[index];
    index += 1;
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(usize::from(separator.width)),
            Style::default().fg(SEPARATOR),
        ))),
        separator,
    );

    let list = chunks[index];
    index += 1;
    render_palette_list(f, view.entries, view.selected, list);

    f.render_widget(Paragraph::new(palette_footer()), chunks[index]);
}

fn bordered(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                title.to_owned(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
}

fn draw_filter(f: &mut Frame, query: &str, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " > ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(query.to_owned(), Style::default().fg(TEXT)),
        ])),
        area,
    );
    let cursor_x = area
        .x
        .saturating_add(3)
        .saturating_add(text_width(query))
        .min(area.x + area.width.saturating_sub(1));
    f.set_cursor_position((cursor_x, area.y));
}

/// The palette's key-hint footer. The `#` hint advertises the numbered-row
/// jump.
pub(crate) fn palette_footer() -> Line<'static> {
    let key_style = Style::default().fg(TEXT).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(DIM);
    Line::from(vec![
        Span::raw(" "),
        Span::styled("↑↓", key_style),
        Span::styled(" navigate  ", dim),
        Span::styled("#", key_style),
        Span::styled(" jump  ", dim),
        Span::styled("Enter", key_style),
        Span::styled(" run  ", dim),
        Span::styled("Esc", key_style),
        Span::styled(" close", dim),
    ])
}

pub(crate) fn render_palette_list(
    f: &mut Frame,
    entries: &[(String, Option<&'static str>)],
    selected: usize,
    area: Rect,
) {
    if entries.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  no matches",
                Style::default().fg(DIM),
            ))),
            area,
        );
        return;
    }
    let height = usize::from(area.height);
    let start = viewport_start(selected, entries.len(), height);
    let active = Style::default().fg(ACTIVE).add_modifier(Modifier::BOLD);
    let inactive = Style::default().fg(TEXT);
    // The shortcut hint stays dim regardless of selection — it's metadata,
    // not part of the focused-row emphasis.
    let hint = Style::default().fg(DIM);
    let lines: Vec<Line<'_>> = entries
        .iter()
        .enumerate()
        .skip(start)
        .take(height.max(1))
        .map(|(i, (label, shortcut))| {
            let (prefix, label_style) = if i == selected {
                (" ▎ ", active)
            } else {
                ("   ", inactive)
            };
            let mut spans = vec![
                Span::styled(prefix, label_style),
                Span::styled(label.clone(), label_style),
            ];
            if let Some(key) = shortcut {
                spans.push(Span::styled(format!("  [{key}]"), hint));
            }
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(count: usize) -> Vec<(String, Option<&'static str>)> {
        (0..count).map(|i| (format!("row {i}"), None)).collect()
    }

    #[test]
    fn a_list_that_fits_never_scrolls() {
        assert_eq!(viewport_start(0, 5, 10), 0);
        assert_eq!(viewport_start(4, 5, 10), 0);
        assert_eq!(viewport_start(4, 5, 5), 0);
    }

    #[test]
    fn a_long_list_scrolls_only_far_enough_to_keep_the_selection_visible() {
        // 60 rows in a 10-row window: nothing moves until the selection
        // reaches the bottom edge, then it tracks one row at a time.
        assert_eq!(viewport_start(9, 60, 10), 0);
        assert_eq!(viewport_start(10, 60, 10), 1);
        assert_eq!(viewport_start(30, 60, 10), 21);
    }

    #[test]
    fn the_viewport_stops_at_the_end_of_the_list() {
        assert_eq!(viewport_start(59, 60, 10), 50);
        // Defensive: a selection past the end can't scroll past the last page.
        assert_eq!(viewport_start(200, 60, 10), 50);
    }

    #[test]
    fn a_zero_height_list_has_nothing_to_scroll() {
        assert_eq!(viewport_start(30, 60, 0), 0);
    }

    #[test]
    fn the_modal_fits_its_widest_row_and_its_footer() {
        let rows = vec![
            ("Mark T123 as complete".to_owned(), Some("^D")),
            ("Go".to_owned(), None),
        ];
        let width = palette_width(&rows, 200);
        let widest = GUTTER + text_width("Mark T123 as complete") + HINT_FRAME + text_width("^D");

        assert!(width >= widest + 2, "{width} must fit {widest} plus borders");
        assert!(width >= FOOTER_MIN);
    }

    #[test]
    fn the_modal_never_outgrows_the_terminal() {
        let rows = vec![("x".repeat(400), Some("^D"))];
        assert_eq!(palette_width(&rows, 80), 80);
        assert_eq!(palette_width(&entries(3), 30), 30);
    }
}
