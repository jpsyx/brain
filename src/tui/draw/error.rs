use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph, Wrap},
};

use crate::render::{ACCENT_RED, TEXT_DIM, TEXT_PRIMARY};

pub(super) fn draw_error(f: &mut Frame, message: &str, can_dismiss: bool, area: Rect) -> Rect {
    let paragraph = Paragraph::new(message)
        .style(Style::default().fg(ACCENT_RED).add_modifier(Modifier::BOLD))
        .block(Block::default().padding(Padding::horizontal(1)))
        .wrap(Wrap { trim: true });
    let height = u16::try_from(paragraph.line_count(area.width.max(1)))
        .unwrap_or(u16::MAX)
        .saturating_add(1)
        .min(area.height);
    let [content, banner] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(height)]).areas(area);
    let [body, hint] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(banner);
    f.render_widget(paragraph, body);
    if can_dismiss {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(TEXT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" dismiss error", Style::default().fg(TEXT_DIM)),
            ])),
            hint,
        );
    }
    content
}
