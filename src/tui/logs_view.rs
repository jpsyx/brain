//! The main-panel diagnostic log view.

use std::path::Path;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::tui::App;

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
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|content| match kind {
                LogKind::Brain => content,
                LogKind::Receiver => content
                    .lines()
                    .filter(|line| line.contains("receiver"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            })
            .filter(|content| !content.is_empty())
            .unwrap_or_else(|| {
                "No verbose log is available for this brain run. Start brain with `--verbose` to collect one.".to_owned()
            });
        Self {
            kind,
            text,
            scroll: 0,
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

pub(crate) fn draw_logs(f: &mut Frame, app: &App<'_>, area: Rect) {
    let Some(logs) = app.logs_view.as_ref() else {
        return;
    };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(78, 92, 122)))
        .title(logs.kind.title());
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(logs.text.as_str())
            .scroll((logs.scroll, 0))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

#[cfg(test)]
mod tests {
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
