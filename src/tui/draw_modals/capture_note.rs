//! The capture-note input modal: one line to name the note, with the hint
//! naming the timestamp an empty submission would title it with.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::render::{ACCENT_CYAN, ACCENT_RED, TEXT_DIM, TEXT_PRIMARY};
use crate::tui::draw::layout::centered_rect;
use crate::tui::modal_state::CaptureNoteState;

pub(crate) fn draw_capture_note(f: &mut Frame, state: &CaptureNoteState, area: Rect) {
    // Wide enough that the hint's timestamp is never the part that falls off
    // the end; narrower terminals wrap it onto the second hint row instead.
    let modal = centered_rect(84.min(area.width), 10.min(area.height), area);
    f.render_widget(Clear, modal);
    let accent = Style::default().fg(ACCENT_CYAN);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(accent)
        .title(Line::from(Span::styled(
            " New capture note ",
            accent.add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(modal);
    f.render_widget(block, modal);
    let chunks = Layout::vertical([
        Constraint::Length(1), // spacer
        Constraint::Length(1), // prompt
        Constraint::Length(1), // input
        Constraint::Length(2), // hint (wraps on a narrow terminal)
        Constraint::Min(0),    // error / spacer
        Constraint::Length(1), // footer
    ])
    .split(inner);

    f.render_widget(
        Paragraph::new(" What do you want to call it?").style(Style::default().fg(TEXT_PRIMARY)),
        chunks[1],
    );

    let text = Line::from(state.buffer());
    let width = usize::from(chunks[2].width.saturating_sub(3));
    let scroll =
        u16::try_from(text.width().saturating_sub(width.saturating_sub(1))).unwrap_or(u16::MAX);
    let input = Rect::new(
        chunks[2].x.saturating_add(3),
        chunks[2].y,
        u16::try_from(width).unwrap_or(0),
        chunks[2].height,
    );
    f.render_widget(Paragraph::new(" > ").style(accent), chunks[2]);
    f.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(TEXT_PRIMARY))
            .scroll((0, scroll)),
        input,
    );
    if input.width > 0 && input.height > 0 {
        let cursor = u16::try_from(Line::from(state.buffer()).width())
            .unwrap_or(u16::MAX)
            .saturating_sub(scroll)
            .min(input.width - 1);
        f.set_cursor_position((input.x.saturating_add(cursor), input.y));
    }

    f.render_widget(
        Paragraph::new(format!(" {}", state.helper_text()))
            .style(Style::default().fg(TEXT_DIM))
            .wrap(Wrap { trim: false }),
        chunks[3],
    );
    if let Some(error) = state.error() {
        f.render_widget(
            Paragraph::new(format!(" {error}")).style(Style::default().fg(ACCENT_RED)),
            chunks[4],
        );
    }

    let key = Style::default()
        .fg(TEXT_PRIMARY)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(TEXT_DIM);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled("Enter", key),
            Span::styled(" create and open  ", dim),
            Span::styled("^U", key),
            Span::styled(" clear  ", dim),
            Span::styled("Esc", key),
            Span::styled(" cancel", dim),
        ])),
        chunks[5],
    );
}
