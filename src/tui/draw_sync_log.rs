//! The live sync-log modal (palette: "Show sync status").
//!
//! Re-reads the running sync's transcript every frame, so the modal tails a
//! sync in progress rather than showing a snapshot taken when it opened. With no
//! sync running it says so and offers nothing else: an earlier run's transcript
//! looks like an answer to "what is happening now?" while answering a different
//! question.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use super::{SyncLogState, centered_rect};

/// Matches the help modal's accent and dim tones.
const ACCENT: ratatui::style::Color = ratatui::style::Color::Rgb(187, 154, 247);
const DIM: ratatui::style::Color = ratatui::style::Color::Rgb(122, 134, 173);

/// What the modal shows for a workspace, given the live log. Pure.
#[must_use]
pub(crate) fn sync_log_body(live: Option<&str>) -> Vec<String> {
    live.map_or_else(
        || vec!["No sync is running right now.".to_owned()],
        |log| {
            let lines: Vec<String> = log.lines().map(str::to_owned).collect();
            if lines.is_empty() {
                vec!["A sync just started; waiting for its first line…".to_owned()]
            } else {
                lines
            }
        },
    )
}

/// The scroll offset that keeps the newest line visible. Pure.
///
/// A log viewer that opens at the top of a growing transcript shows the least
/// interesting part, so an untouched modal follows the tail.
#[must_use]
pub(crate) fn tail_scroll(total: usize, visible: u16, requested: u16) -> u16 {
    let total = u16::try_from(total).unwrap_or(u16::MAX);
    let max = total.saturating_sub(visible);
    requested.min(max)
}

pub(crate) fn draw_sync_log(f: &mut Frame, state: &SyncLogState, live: Option<&str>, area: Rect) {
    let body = sync_log_body(live);
    let width = 90.min(area.width);
    let height = area.height.saturating_sub(4).max(6);
    let modal = centered_rect(width, height, area);
    f.render_widget(Clear, modal);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Sync status",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .title_bottom(Line::from(Span::styled(
            " j/k scroll · Esc closes ",
            Style::default().fg(DIM),
        )));
    let visible = modal.height.saturating_sub(2);
    let scroll = tail_scroll(body.len(), visible, state.scroll);
    let text: Vec<Line> = body.into_iter().map(Line::from).collect();
    f.render_widget(Paragraph::new(text).block(block).scroll((scroll, 0)), modal);
}

#[cfg(test)]
mod tests {
    use super::{sync_log_body, tail_scroll};

    #[test]
    fn with_no_running_sync_the_modal_says_exactly_that() {
        assert_eq!(
            sync_log_body(None),
            ["No sync is running right now.".to_owned()]
        );
    }

    #[test]
    fn a_running_sync_shows_its_transcript_line_by_line() {
        assert_eq!(
            sync_log_body(Some("syncing now (pull)\n\nProbing…\n  found: nothing\n")),
            [
                "syncing now (pull)".to_owned(),
                String::new(),
                "Probing…".to_owned(),
                "  found: nothing".to_owned(),
            ]
        );
    }

    #[test]
    fn a_sync_with_no_lines_yet_says_it_is_starting() {
        assert_eq!(
            sync_log_body(Some("")),
            ["A sync just started; waiting for its first line…".to_owned()]
        );
    }

    #[test]
    fn an_untouched_modal_follows_the_tail() {
        // 40 lines, 10 visible: the newest line must be on screen.
        assert_eq!(tail_scroll(40, 10, u16::MAX), 30);
    }

    #[test]
    fn a_short_log_never_scrolls() {
        assert_eq!(tail_scroll(4, 10, u16::MAX), 0);
        assert_eq!(tail_scroll(4, 10, 0), 0);
    }

    #[test]
    fn an_explicit_scroll_within_range_is_respected() {
        assert_eq!(tail_scroll(40, 10, 5), 5);
    }
}
