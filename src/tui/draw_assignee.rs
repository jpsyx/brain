//! Drawing the shared-workspace assignee filter picker.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::tui::draw::layout::centered_rect;
use crate::tui::modal_state::AssigneeFilterState;

pub(crate) fn draw_assignee_filter(f: &mut Frame, state: &AssigneeFilterState, area: Rect) {
    let rows = state.rows();
    let list_h = u16::try_from(rows.len()).unwrap_or(u16::MAX);
    let height = list_h.saturating_add(4).max(7).min(area.height);
    let modal = centered_rect(62.min(area.width), height, area);
    f.render_widget(Clear, modal);

    let accent = Color::Rgb(125, 207, 255);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Filter by assignee",
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    f.render_widget(
        Paragraph::new(" Choose a portable workspace member"),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(usize::from(chunks[1].width)),
            Style::default().fg(Color::Rgb(78, 92, 122)),
        ))),
        chunks[1],
    );

    let active = Style::default()
        .fg(Color::Rgb(255, 199, 119))
        .add_modifier(Modifier::BOLD);
    let inactive = Style::default().fg(Color::Rgb(192, 202, 245));
    let list: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let style = if index == state.selected() {
                active
            } else {
                inactive
            };
            let prefix = if index == state.selected() {
                " ▎ "
            } else {
                "   "
            };
            Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(format!("{}. {label}", index + 1), style),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(list), chunks[2]);

    let dim = Style::default().fg(Color::Rgb(122, 134, 173));
    let key = Style::default()
        .fg(Color::Rgb(192, 202, 245))
        .add_modifier(Modifier::BOLD);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled("↑↓", key),
            Span::styled(" navigate  ", dim),
            Span::styled("#", key),
            Span::styled(" apply  ", dim),
            Span::styled("Enter", key),
            Span::styled(" apply  ", dim),
            Span::styled("Esc", key),
            Span::styled(" close", dim),
        ])),
        chunks[3],
    );
}
