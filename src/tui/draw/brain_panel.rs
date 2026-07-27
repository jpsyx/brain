//! Rendering the brain panel: the bordered agent PTY (via `tui-term`), its
//! focus-aware title/border, cursor placement, and the footer that shows the
//! resume alert or the normal hint.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use tui_term::widget::PseudoTerminal;

use crate::pty_pane::PtyPane;
use crate::tui::*;

pub(crate) fn draw_brain(f: &mut Frame, app: &mut App<'_>, area: Rect) {
    let focused = app.focus == Panel::Brain;
    let alive = app.brain.as_ref().is_some_and(PtyPane::is_alive);

    let border_color = if focused {
        Color::Rgb(125, 207, 255) // cyan accent — matches the rest of the palette
    } else {
        Color::Rgb(78, 92, 122) // very dim
    };
    let agent = app.agent_kind.label();
    let title_status = if alive {
        format!("Brain · {agent}")
    } else {
        format!("Brain · {agent} exited")
    };
    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled(
            title_status,
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ]);
    let block = Block::default()
        .borders(Borders::LEFT | Borders::TOP)
        .border_style(Style::default().fg(border_color))
        .title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Reserve the bottom row for the help / status footer.
    let term_h = inner.height.saturating_sub(1);
    let term_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: term_h,
    };
    let footer_area = Rect {
        x: inner.x,
        y: inner.y + term_h,
        width: inner.width,
        height: 1,
    };

    // Resize the PTY + parser to match the inner terminal area. No-op when
    // dimensions match, so this is safe to call every frame.
    if let Some(pty) = app.brain.as_mut() {
        if term_area.height > 0 && term_area.width > 0 {
            pty.resize(term_area.height, term_area.width);
        }
    }

    if let Some(pty) = app.brain.as_ref() {
        if let Ok(parser) = pty.parser.read() {
            let screen = parser.screen();
            let widget = PseudoTerminal::new(screen);
            f.render_widget(widget, term_area);

            // Place the real terminal cursor over the inner cursor when the
            // brain panel is focused; otherwise leave it on the tasks side.
            if focused && alive && !screen.hide_cursor() {
                let (row, col) = screen.cursor_position();
                let cx = term_area.x.saturating_add(col);
                let cy = term_area.y.saturating_add(row);
                f.set_cursor_position((cx, cy));
            }
        }
    }

    // Footer: a startup alert (resume failed → fresh chat) takes the row in
    // amber until the user switches focus; otherwise the normal hint shows.
    let key = Style::default()
        .fg(Color::Rgb(192, 202, 245))
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::Rgb(122, 134, 173));
    let footer = match &app.alert {
        Some(alert) => Line::from(Span::styled(
            format!(" {alert}"),
            Style::default()
                .fg(Color::Rgb(255, 199, 119))
                .add_modifier(Modifier::BOLD),
        )),
        None if alive => Line::from(vec![
            Span::raw(" "),
            Span::styled("Alt+H", key),
            Span::styled(" tasks", dim),
            Span::styled("   ", dim),
            Span::styled("^X", key),
            Span::styled(" close brain", dim),
        ]),
        // The event loop closes the panel as soon as the agent exits, so this
        // shows for at most one frame before tasks goes full-width.
        None => Line::from(Span::styled(
            format!(" {agent} exited: closing panel..."),
            Style::default()
                .fg(Color::Rgb(255, 199, 119))
                .add_modifier(Modifier::BOLD),
        )),
    };
    f.render_widget(Paragraph::new(vec![footer]), footer_area);
}
