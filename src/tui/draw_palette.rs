//! Drawing the command-palette / task-actions modal.

use super::*;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

pub(crate) fn draw_palette(f: &mut Frame, state: &TaskPalette, area: Rect) {
    let visible = state.visible();
    let show_subtitle = state.task_actions_modal() && state.subtitle().is_some();
    // Base inner height: filter + separator + list (≥1) + footer = 4 +
    // visible. Add a row when the task actions modal subtitle is shown.
    let list_h = u16::try_from(visible.len()).unwrap_or(u16::MAX);
    let extra = u16::from(show_subtitle);
    let height = list_h
        .saturating_add(5)
        .saturating_add(extra)
        .max(8 + extra)
        .min(area.height);
    let modal = centered_rect(60.min(area.width), height, area);
    f.render_widget(Clear, modal);

    let accent = Color::Rgb(187, 154, 247);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                state.title(),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    // Vertical layout. The subtitle row is conditional: when the
    // task actions modal is open we render the dimmed task label below
    // the title (matching the confirm-modal pattern), otherwise we go
    // straight to the filter input.
    let mut constraints: Vec<Constraint> = Vec::with_capacity(5);
    if show_subtitle {
        constraints.push(Constraint::Length(1)); // task label (dim)
    }
    constraints.push(Constraint::Length(1)); // filter
    constraints.push(Constraint::Length(1)); // separator
    constraints.push(Constraint::Min(1)); // list
    constraints.push(Constraint::Length(1)); // footer hint
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let mut idx = 0_usize;
    if show_subtitle {
        let label = state.subtitle().unwrap_or("");
        // Truncate to fit; small left padding keeps the label off the
        // border.
        let max_chars = usize::from(inner.width).saturating_sub(2).max(8);
        let label_text = crate::tasks::render::truncate(label, max_chars);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(label_text, Style::default().fg(Color::Rgb(122, 134, 173))),
            ])),
            chunks[idx],
        );
        idx += 1;
    }

    // Filter input.
    let filter_area = chunks[idx];
    idx += 1;
    let prompt = Line::from(vec![
        Span::styled(
            " > ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            state.query().to_owned(),
            Style::default().fg(Color::Rgb(192, 202, 245)),
        ),
    ]);
    f.render_widget(Paragraph::new(prompt), filter_area);
    let cursor_x = filter_area
        .x
        .saturating_add(3)
        .saturating_add(u16::try_from(state.query().chars().count()).unwrap_or(u16::MAX))
        .min(filter_area.x + filter_area.width.saturating_sub(1));
    f.set_cursor_position((cursor_x, filter_area.y));

    // Separator.
    let sep_area = chunks[idx];
    idx += 1;
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(usize::from(sep_area.width)),
            Style::default().fg(Color::Rgb(78, 92, 122)),
        ))),
        sep_area,
    );

    let list_area = chunks[idx];
    idx += 1;
    let entries = state.numbered_entries();
    render_palette_list(f, &entries, state.selected(), list_area);

    f.render_widget(Paragraph::new(palette_footer()), chunks[idx]);
}

/// The palette's key-hint footer. Extracted so `draw_palette` stays under
/// the line cap; the `#` hint advertises the numbered-row jump.
pub(crate) fn palette_footer() -> Line<'static> {
    let key_style = Style::default()
        .fg(Color::Rgb(192, 202, 245))
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::Rgb(122, 134, 173));
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
                Style::default().fg(Color::Rgb(122, 134, 173)),
            ))),
            area,
        );
        return;
    }
    let active = Style::default()
        .fg(Color::Rgb(255, 199, 119))
        .add_modifier(Modifier::BOLD);
    let inactive = Style::default().fg(Color::Rgb(192, 202, 245));
    // Shortcut hint stays dim regardless of selection — it's metadata,
    // not part of the focused-row emphasis.
    let hint = Style::default().fg(Color::Rgb(122, 134, 173));
    let lines: Vec<Line<'_>> = entries
        .iter()
        .enumerate()
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
