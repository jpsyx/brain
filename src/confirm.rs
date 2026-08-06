//! The yes/no confirmation modal.
//!
//! A small overlay shown before a per-file action that wants a deliberate
//! answer. Two flavors, distinguished by [`ConfirmKind`]:
//!
//! - **Create PDF** (`Ctrl-G` on a markdown file) — green chrome, defaults to
//!   **Yes** (the action was already requested).
//! - **Delete** (`Ctrl-D` on any entry) — red chrome, defaults to **No**
//!   because it's destructive; the file is moved to the Trash on Yes.
//!
//! Like the command palette it is **not** a screen of its own: the picker /
//! two-panel TUI holds a `Confirm` in its state, routes keys to [`handle_key`],
//! and paints it with [`draw_modal`] as a centered overlay. `Enter` confirms
//! the highlighted button, `←`/`→`/`Tab`/`h`/`l` swap buttons, `y`/`n` answer
//! directly, and `Esc` / `Ctrl-c` cancel.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::render;

/// Which action the modal confirms. Drives the chrome color, title, question,
/// and the default-highlighted button.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Convert a markdown file to a colocated PDF (constructive; green).
    Pdf,
    /// Move a file or directory to the Trash (destructive; red).
    Delete,
}

/// State for the confirmation overlay: the target path, which action it
/// confirms, and which button (Yes / No) is highlighted.
pub struct Confirm {
    /// The target path. The host reads this on `Accept`.
    pub path: PathBuf,
    /// Which action this modal confirms.
    pub kind: ConfirmKind,
    /// `true` when the Yes button is highlighted.
    yes: bool,
}

impl Confirm {
    /// Open a "Create PDF" confirmation, defaulting to Yes (the action was
    /// already requested via `Ctrl-G`, so Yes is the expected answer).
    #[must_use]
    pub const fn pdf(path: PathBuf) -> Self {
        Self {
            path,
            kind: ConfirmKind::Pdf,
            yes: true,
        }
    }

    /// Open a "Delete" confirmation, defaulting to **No**: deleting is
    /// destructive, so the safe answer is highlighted and a stray `Enter`
    /// cancels rather than deletes.
    #[must_use]
    pub const fn delete(path: PathBuf) -> Self {
        Self {
            path,
            kind: ConfirmKind::Delete,
            yes: false,
        }
    }

    const fn toggle(&mut self) {
        self.yes = !self.yes;
    }
}

/// What a keypress asks the host to do with the modal.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Keep the modal open.
    Continue,
    /// Dismiss without acting (No / Esc / Ctrl-c).
    Cancel,
    /// Confirm: perform the action.
    Accept,
}

/// Pure key handling for the modal. `Enter` confirms the highlighted button;
/// `←`/`→`/`Tab`/`h`/`l` swap buttons; `y`/`n` answer directly; `Esc` /
/// `Ctrl-c` cancel.
pub const fn handle_key(c: &mut Confirm, k: KeyEvent) -> Step {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Esc | KeyCode::Char('n' | 'N') => Step::Cancel,
        KeyCode::Char('c') if ctrl => Step::Cancel,
        KeyCode::Char('y' | 'Y') => Step::Accept,
        KeyCode::Enter => {
            if c.yes {
                Step::Accept
            } else {
                Step::Cancel
            }
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::Char('h' | 'l') => {
            c.toggle();
            Step::Continue
        }
        _ => Step::Continue,
    }
}

/// The accent color for a modal kind: green for the constructive PDF action,
/// red for the destructive delete.
const fn accent(kind: ConfirmKind) -> Color {
    match kind {
        ConfirmKind::Pdf => render::ACCENT_GREEN,
        ConfirmKind::Delete => render::ACCENT_RED,
    }
}

/// The modal's title bar text.
const fn title(kind: ConfirmKind) -> &'static str {
    match kind {
        ConfirmKind::Pdf => "Create PDF",
        ConfirmKind::Delete => "Delete",
    }
}

/// The question shown in the body, naming the target file.
fn question(kind: ConfirmKind, filename: &str) -> String {
    match kind {
        ConfirmKind::Pdf => format!("Would you like to create a PDF for '{filename}'?"),
        ConfirmKind::Delete => format!("Delete '{filename}'? It moves to the Trash."),
    }
}

/// The filename shown in the modal question (just the file's name, not the
/// full path). Falls back to the path's display if it has no file name.
fn modal_filename(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Render the confirmation modal as a centered overlay, chromed by kind
/// (green for PDF, red for delete).
pub fn draw_modal(f: &mut Frame, c: &Confirm, area: Rect) {
    let accent = accent(c.kind);
    let question = question(c.kind, &modal_filename(&c.path));

    // Grow to fit the question, clamped to the available area.
    let width = u16::try_from(question.chars().count() + 4)
        .unwrap_or(u16::MAX)
        .clamp(24, area.width.max(1));
    let height = 5u16.min(area.height.max(1));
    let modal = centered_rect(width, height, area);

    f.render_widget(Clear, modal);
    let title_style = Style::new().fg(accent).add_modifier(Modifier::BOLD);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(accent))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(title(c.kind), title_style),
            Span::raw(" "),
        ]));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // question
            Constraint::Min(1),    // spacer
            Constraint::Length(1), // buttons
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            question,
            Style::new().fg(render::TEXT_PRIMARY),
        ))),
        chunks[0],
    );
    f.render_widget(Paragraph::new(buttons_line(c.yes, accent)), chunks[2]);
}

/// The Yes / No button row, highlighting the selected one on the accent fill.
fn buttons_line(yes: bool, accent: Color) -> Line<'static> {
    let selected = Style::new()
        .fg(render::SELECTED_BG)
        .bg(accent)
        .add_modifier(Modifier::BOLD);
    let idle = Style::new().fg(render::TEXT_DIM);
    Line::from(vec![
        Span::raw("  "),
        Span::styled("  Yes  ", if yes { selected } else { idle }),
        Span::raw("   "),
        Span::styled("  No  ", if yes { idle } else { selected }),
    ])
}

/// A `width`×`height` rectangle centered within `area` (clamped to it).
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn confirm() -> Confirm {
        Confirm::pdf(PathBuf::from("/a/b/plan.md"))
    }

    #[test]
    fn pdf_defaults_to_yes_so_enter_accepts() {
        let mut c = confirm();
        assert_eq!(handle_key(&mut c, key(KeyCode::Enter)), Step::Accept);
    }

    #[test]
    fn delete_defaults_to_no_so_a_stray_enter_cancels() {
        // Destructive: the safe answer is highlighted, so Enter cancels and
        // deleting takes a deliberate `y` or a toggle first.
        let mut c = Confirm::delete(PathBuf::from("/a/b/old.md"));
        assert_eq!(handle_key(&mut c, key(KeyCode::Enter)), Step::Cancel);
        // But an explicit `y` still deletes.
        let mut c = Confirm::delete(PathBuf::from("/a/b/old.md"));
        assert_eq!(handle_key(&mut c, key(KeyCode::Char('y'))), Step::Accept);
    }

    #[test]
    fn toggling_to_no_makes_enter_cancel() {
        let mut c = confirm();
        assert_eq!(handle_key(&mut c, key(KeyCode::Right)), Step::Continue);
        assert_eq!(handle_key(&mut c, key(KeyCode::Enter)), Step::Cancel);
    }

    #[test]
    fn tab_and_arrows_and_hl_all_toggle() {
        for code in [
            KeyCode::Tab,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Char('h'),
            KeyCode::Char('l'),
        ] {
            let mut c = confirm();
            assert_eq!(handle_key(&mut c, key(code)), Step::Continue);
            // One toggle flips the default Yes → No.
            assert_eq!(handle_key(&mut c, key(KeyCode::Enter)), Step::Cancel);
        }
    }

    #[test]
    fn y_and_n_answer_directly_regardless_of_highlight() {
        let mut c = confirm();
        assert_eq!(handle_key(&mut c, key(KeyCode::Char('y'))), Step::Accept);
        let mut c = confirm();
        assert_eq!(handle_key(&mut c, key(KeyCode::Char('n'))), Step::Cancel);
    }

    #[test]
    fn esc_and_ctrl_c_cancel() {
        let mut c = confirm();
        assert_eq!(handle_key(&mut c, key(KeyCode::Esc)), Step::Cancel);
        let mut c = confirm();
        assert_eq!(handle_key(&mut c, ctrl(KeyCode::Char('c'))), Step::Cancel);
    }

    #[test]
    fn selected_button_uses_the_accent_fill() {
        // Yes highlighted → the "Yes" span carries the accent background.
        let line = buttons_line(true, render::ACCENT_GREEN);
        let yes = line
            .spans
            .iter()
            .find(|s| s.content.contains("Yes"))
            .expect("a Yes span");
        assert_eq!(yes.style.bg, Some(render::ACCENT_GREEN));
        // No highlighted → the "No" span does.
        let line = buttons_line(false, render::ACCENT_GREEN);
        let no = line
            .spans
            .iter()
            .find(|s| s.content.contains("No"))
            .expect("a No span");
        assert_eq!(no.style.bg, Some(render::ACCENT_GREEN));
    }

    #[test]
    fn each_kind_carries_its_own_accent_title_and_question() {
        assert_eq!(accent(ConfirmKind::Pdf), render::ACCENT_GREEN);
        assert_eq!(accent(ConfirmKind::Delete), render::ACCENT_RED);
        assert_eq!(title(ConfirmKind::Pdf), "Create PDF");
        assert_eq!(title(ConfirmKind::Delete), "Delete");
        assert!(question(ConfirmKind::Pdf, "plan.md").contains("create a PDF"));
        let del = question(ConfirmKind::Delete, "plan.md");
        assert!(del.contains("Delete 'plan.md'"));
        assert!(del.contains("Trash"));
    }

    #[test]
    fn modal_filename_is_just_the_file_name() {
        assert_eq!(modal_filename(Path::new("/a/b/plan.md")), "plan.md");
    }
}
