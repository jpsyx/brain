//! The main-panel diagnostic log view.

use std::path::Path;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LogKind {
    Receiver,
    Brain,
}

impl LogKind {
    fn title(self) -> &'static str {
        match self {
            Self::Receiver => "Receiver logs",
            Self::Brain => "Brain TUI logs",
        }
    }
}

pub(crate) struct LogsView {
    pub(crate) kind: LogKind,
    pub(crate) text: String,
    pub(crate) scroll: u16,
}

impl LogsView {
    pub(crate) fn load(kind: LogKind, path: Option<&Path>) -> Self {
        let text = path
            .and_then(|path| match kind {
                LogKind::Brain => crate::logging::export::recent_logs(path).ok(),
                LogKind::Receiver => std::fs::read_to_string(path).ok(),
            })
            .map(|content| match kind {
                LogKind::Brain => content,
                LogKind::Receiver => content
                    .lines()
                    .filter(|line| line.contains("receiver"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            })
            .filter(|content| !content.is_empty())
            .unwrap_or_else(|| "No log entries are available for this brain run yet.".to_owned());
        Self {
            kind,
            text,
            scroll: u16::MAX,
        }
    }

    pub(crate) fn scroll_by(&mut self, amount: i16) {
        if amount.is_negative() {
            self.scroll = self.scroll.saturating_sub(amount.unsigned_abs());
        } else {
            self.scroll = self.scroll.saturating_add(amount.unsigned_abs());
        }
    }
}

pub(crate) fn draw_logs(f: &mut Frame, logs: Option<&mut LogsView>, area: Rect) {
    let Some(logs) = logs else {
        return;
    };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(78, 92, 122)))
        .title(logs.kind.title());
    let inner = block.inner(area);
    f.render_widget(block, area);
    let paragraph = Paragraph::new(logs.text.as_str()).wrap(Wrap { trim: false });
    let last_page = paragraph
        .line_count(inner.width)
        .saturating_sub(usize::from(inner.height));
    logs.scroll = logs
        .scroll
        .min(u16::try_from(last_page).unwrap_or(u16::MAX));
    f.render_widget(paragraph.scroll((logs.scroll, 0)), inner);
}

#[cfg(test)]
mod tests {
    #[test]
    fn opening_logs_shows_the_latest_full_error_in_a_narrow_view() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("run.log");
        let mut logger = crate::logging::Logger::open(&path).unwrap();
        for _ in 0..100_000 {
            logger.write("old event").unwrap();
        }
        logger
            .write("launch failed: receive_pending [receive]: complete the interrupted sync")
            .unwrap();
        let mut logs = super::LogsView::load(super::LogKind::Brain, Some(&path));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 8)).unwrap();
        terminal
            .draw(|frame| super::draw_logs(frame, Some(&mut logs), frame.area()))
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(screen.contains("receive_pending"), "{screen}");
        assert!(screen.contains("interrupted sync"), "{screen}");
    }

    #[test]
    fn receiver_logs_filter_out_unrelated_brain_lines() {
        let content = "brain start\nreceiver server started\nsync complete\nreceiver request sms";
        let filtered = content
            .lines()
            .filter(|line| line.contains("receiver"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(filtered, "receiver server started\nreceiver request sms");
    }
}
