use super::{SharePhase, ShareState};
use crate::render::{ACCENT_CYAN, ACCENT_RED, TEXT_DIM, TEXT_PRIMARY};
use crate::tui::draw::layout::centered_rect;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

pub(crate) fn draw(frame: &mut Frame, state: &ShareState, area: Rect) {
    let modal = centered_rect(94.min(area.width), 18.min(area.height), area);
    frame.render_widget(Clear, modal);
    let accent = Style::default().fg(ACCENT_CYAN);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(accent)
        .title(" Share a download link ");
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    let rows = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(inner);
    let (lines, footer) = match &state.phase {
        SharePhase::Path => (
            vec![
                Line::from(""),
                Line::from("File or directory to share"),
                Line::from(""),
                input(state),
                dim("Relative paths start at the current working directory."),
            ],
            "Enter choose path   Ctrl+U clear   Esc cancel",
        ),
        SharePhase::Expiration(path) => (
            vec![
                Line::from(""),
                Line::from(path.display().to_string()),
                Line::from(""),
                Line::from("How long should the link work?"),
                input(state),
                dim("Blank defaults to 7 days. Examples: 1h, 12 hours, 3 days."),
                dim("Backblaze allows 1 second through 7 days."),
                dim("Directories become a ZIP inside the selected directory."),
            ],
            "Enter share   Ctrl+U clear   Esc cancel",
        ),
        SharePhase::Running(_) => (
            vec![
                Line::from(""),
                Line::from(state.progress.clone()),
                Line::from(""),
                dim("The link will be copied when ready."),
            ],
            "Esc cancel sharing",
        ),
        SharePhase::Ready(result) => (
            vec![
                Line::from(""),
                Line::styled(
                    if state.copied {
                        "Download link copied to clipboard"
                    } else {
                        "Download link ready"
                    },
                    accent,
                ),
                Line::from(result.path.display().to_string()),
                Line::from(format!("Expires: {}", result.expires_at)),
                Line::from(""),
                Line::from(result.url.clone()),
                Line::from(""),
                dim("Anyone with this link can download until it expires."),
            ],
            "Enter copy link   Esc close",
        ),
    };
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().fg(TEXT_PRIMARY))
            .wrap(Wrap { trim: false }),
        rows[0],
    );
    if let Some(error) = &state.error {
        frame.render_widget(
            Paragraph::new(error.as_str())
                .style(Style::default().fg(ACCENT_RED))
                .wrap(Wrap { trim: false }),
            rows[1],
        );
    }
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(TEXT_DIM)),
        rows[2],
    );
}
fn dim(text: &str) -> Line<'_> {
    Line::styled(text, Style::default().fg(TEXT_DIM))
}
fn input(state: &ShareState) -> Line<'_> {
    Line::from(vec![
        Span::styled("> ", Style::default().fg(ACCENT_CYAN)),
        Span::raw(&state.buffer),
        Span::styled("▏", Style::default().fg(ACCENT_CYAN)),
    ])
}
