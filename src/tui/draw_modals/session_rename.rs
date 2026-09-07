use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::render::{
    ACCENT_CYAN, ACCENT_YELLOW, SELECTED_BG, TEXT_DIM, TEXT_PRIMARY, TEXT_VERY_DIM,
};
use crate::tui::draw::layout::centered_rect;
use crate::tui::modal_state::SessionRenamePickerState;

pub(crate) fn draw_session_rename_picker(
    frame: &mut Frame,
    state: &SessionRenamePickerState,
    area: Rect,
) {
    let height = u16::try_from(state.rows().len())
        .unwrap_or(u16::MAX)
        .saturating_add(5)
        .min(area.height);
    let modal = centered_rect(70.min(area.width), height, area);
    frame.render_widget(Clear, modal);
    let accent = Style::new().fg(ACCENT_CYAN);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(accent)
        .title(Line::from(Span::styled(
            " Rename session ",
            accent.add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(" Select a session to rename").style(Style::new().fg(TEXT_DIM)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(inner.width))).style(Style::new().fg(TEXT_VERY_DIM)),
        chunks[1],
    );
    let lines: Vec<Line<'static>> = state
        .rows()
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let selected = index == state.selected();
            let marker = if selected { " ▎ " } else { "   " };
            let selected_style = |style: Style| {
                if selected {
                    style.bg(SELECTED_BG)
                } else {
                    style
                }
            };
            let marker_style = selected_style(Style::new().fg(ACCENT_CYAN));
            let title_style = if row.renameable {
                selected_style(Style::new().fg(TEXT_PRIMARY))
            } else {
                selected_style(
                    Style::new()
                        .fg(TEXT_DIM)
                        .add_modifier(Modifier::CROSSED_OUT),
                )
            };
            let mut spans = vec![
                Span::styled(marker, marker_style),
                Span::styled(format!("{}. {}", index + 1, row.title), title_style),
            ];
            if !row.renameable {
                spans.push(Span::styled(
                    "  [not renameable]",
                    selected_style(Style::new().fg(ACCENT_YELLOW)),
                ));
            }
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), chunks[2]);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " ↑↓",
                Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" navigate  ", Style::new().fg(TEXT_DIM)),
            Span::styled(
                "Enter",
                Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" rename  ", Style::new().fg(TEXT_DIM)),
            Span::styled(
                "Esc",
                Style::new().fg(TEXT_PRIMARY).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" cancel", Style::new().fg(TEXT_DIM)),
        ])),
        chunks[3],
    );
}
