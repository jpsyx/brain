//! An interactive toggle-checklist for choosing a set of items (namespaces,
//! task tags) during onboarding and `brain config set`.
//!
//! Everything here is a **pure state machine** (`Checklist` + `handle_key`),
//! tested by feeding synthetic key events and asserting state — the same
//! pure/impure split the menu uses. The raw-mode `/dev/tty` rendering lives in
//! [`run`], a thin shell.
//!
//! Items start all-checked (the "default to all selected" rule). Space toggles
//! the row under the cursor; `a` opens a free-text line to *create new* items,
//! parsed tolerantly (commas/semicolons/whitespace, per-item normalize, dedupe);
//! Enter confirms; Esc cancels.

pub mod run;

use std::collections::BTreeSet;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Run a checklist interactively (all items start checked) and return the
/// chosen set. `Ok(None)` on cancel or when there is no terminal. Thin wrapper
/// over the pure state machine + the `/dev/tty` shell.
pub fn choose(
    title: impl Into<String>,
    initial: &[String],
    normalize: fn(&str) -> Option<String>,
) -> Result<Option<Vec<String>>> {
    run::run_checklist(Checklist::new(title, initial, normalize))
}

/// One row: a label and whether it's selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub checked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Browse,
    /// Typing new items into a free-text buffer.
    Create(String),
}

/// What a key press resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Stay in the widget.
    Continue,
    /// The user accepted the current selection (Enter in browse mode).
    Confirm,
    /// The user aborted (Esc/q/Ctrl-C in browse mode).
    Cancel,
}

/// The checklist state: the rows, the cursor, the input mode, and the
/// per-item normalizer used when creating new entries.
pub struct Checklist {
    pub title: String,
    pub items: Vec<Item>,
    pub cursor: usize,
    mode: Mode,
    normalize: fn(&str) -> Option<String>,
}

impl Checklist {
    /// Build a checklist whose rows are `initial` (order preserved), all
    /// checked. `normalize` canonicalizes tokens typed via *create new*.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        initial: &[String],
        normalize: fn(&str) -> Option<String>,
    ) -> Self {
        let items = initial
            .iter()
            .map(|l| Item {
                label: l.clone(),
                checked: true,
            })
            .collect();
        Self {
            title: title.into(),
            items,
            cursor: 0,
            mode: Mode::Browse,
            normalize,
        }
    }

    /// The current *create new* buffer, if creating.
    #[must_use]
    pub fn create_buffer(&self) -> Option<&str> {
        match &self.mode {
            Mode::Create(s) => Some(s),
            Mode::Browse => None,
        }
    }

    /// The selected labels, in row order.
    #[must_use]
    pub fn result(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| i.checked)
            .map(|i| i.label.clone())
            .collect()
    }

    /// Advance the state machine by one key. Returns whether to continue,
    /// confirm, or cancel.
    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        match &self.mode {
            Mode::Browse => self.handle_browse(key),
            Mode::Create(_) => {
                self.handle_create(key);
                Outcome::Continue
            }
        }
    }

    fn handle_browse(&mut self, key: KeyEvent) -> Outcome {
        let ctrl_c =
            key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            _ if ctrl_c => Outcome::Cancel,
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                Outcome::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.items.is_empty() {
                    self.cursor = (self.cursor + 1).min(self.items.len() - 1);
                }
                Outcome::Continue
            }
            KeyCode::Char(' ') => {
                if let Some(it) = self.items.get_mut(self.cursor) {
                    it.checked = !it.checked;
                }
                Outcome::Continue
            }
            KeyCode::Char('a') => {
                self.mode = Mode::Create(String::new());
                Outcome::Continue
            }
            KeyCode::Enter => Outcome::Confirm,
            KeyCode::Esc | KeyCode::Char('q') => Outcome::Cancel,
            _ => Outcome::Continue,
        }
    }

    fn handle_create(&mut self, key: KeyEvent) {
        // Take ownership of the buffer to satisfy the borrow checker.
        let Mode::Create(mut buf) = std::mem::replace(&mut self.mode, Mode::Browse) else {
            return;
        };
        match key.code {
            KeyCode::Enter => {
                self.commit_created(&buf);
                self.mode = Mode::Browse;
            }
            KeyCode::Esc => {
                self.mode = Mode::Browse; // discard
            }
            KeyCode::Backspace => {
                buf.pop();
                self.mode = Mode::Create(buf);
            }
            KeyCode::Char(c) => {
                buf.push(c);
                self.mode = Mode::Create(buf);
            }
            _ => {
                self.mode = Mode::Create(buf);
            }
        }
    }

    /// Parse the create buffer and append new, deduped, normalized items
    /// (checked). Existing labels are left untouched (no duplicates).
    fn commit_created(&mut self, buf: &str) {
        let existing: BTreeSet<String> = self.items.iter().map(|i| i.label.clone()).collect();
        let mut added: BTreeSet<String> = BTreeSet::new();
        for raw in buf.split([',', ';', ' ', '\t', '\n', '\r']) {
            if raw.is_empty() {
                continue;
            }
            if let Some(item) = (self.normalize)(raw) {
                if !existing.contains(&item) && added.insert(item.clone()) {
                    self.items.push(Item {
                        label: item,
                        checked: true,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn ident(s: &str) -> Option<String> {
        let t = s.trim().to_lowercase();
        (!t.is_empty()).then_some(t)
    }

    fn sample() -> Checklist {
        Checklist::new(
            "Namespaces",
            &["work".to_owned(), "personal".to_owned(), "pole".to_owned()],
            ident,
        )
    }

    #[test]
    fn starts_all_checked_in_order() {
        let c = sample();
        assert!(c.items.iter().all(|i| i.checked));
        assert_eq!(c.result(), ["work", "personal", "pole"]);
    }

    #[test]
    fn space_toggles_the_row_under_the_cursor() {
        let mut c = sample();
        assert_eq!(c.handle_key(key(KeyCode::Char(' '))), Outcome::Continue);
        // "work" (cursor 0) is now unchecked.
        assert_eq!(c.result(), ["personal", "pole"]);
        c.handle_key(key(KeyCode::Char(' '))); // toggle back on
        assert_eq!(c.result(), ["work", "personal", "pole"]);
    }

    #[test]
    fn cursor_moves_and_clamps() {
        let mut c = sample();
        c.handle_key(key(KeyCode::Up)); // clamps at 0
        assert_eq!(c.cursor, 0);
        c.handle_key(key(KeyCode::Down));
        c.handle_key(key(KeyCode::Down));
        c.handle_key(key(KeyCode::Down)); // clamps at last
        assert_eq!(c.cursor, 2);
        // Toggle the last row off.
        c.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(c.result(), ["work", "personal"]);
    }

    #[test]
    fn create_new_parses_tolerantly_dedupes_and_appends_checked() {
        let mut c = sample();
        c.handle_key(key(KeyCode::Char('a')));
        assert!(c.create_buffer().is_some());
        for ch in "avandar,, work; side project".chars() {
            c.handle_key(key(KeyCode::Char(ch)));
        }
        assert_eq!(c.create_buffer(), Some("avandar,, work; side project"));
        c.handle_key(key(KeyCode::Enter));
        assert!(c.create_buffer().is_none());
        // "work" already existed (no dup); "avandar", "side", "project" added.
        assert_eq!(
            c.result(),
            ["work", "personal", "pole", "avandar", "side", "project"]
        );
    }

    #[test]
    fn create_escape_discards_the_buffer() {
        let mut c = sample();
        c.handle_key(key(KeyCode::Char('a')));
        c.handle_key(key(KeyCode::Char('x')));
        c.handle_key(key(KeyCode::Esc));
        assert!(c.create_buffer().is_none());
        assert_eq!(c.result(), ["work", "personal", "pole"]); // unchanged
    }

    #[test]
    fn backspace_edits_the_create_buffer() {
        let mut c = sample();
        c.handle_key(key(KeyCode::Char('a')));
        for ch in "abz".chars() {
            c.handle_key(key(KeyCode::Char(ch)));
        }
        c.handle_key(key(KeyCode::Backspace));
        assert_eq!(c.create_buffer(), Some("ab"));
    }

    #[test]
    fn enter_confirms_and_esc_cancels_in_browse() {
        let mut c = sample();
        assert_eq!(c.handle_key(key(KeyCode::Enter)), Outcome::Confirm);
        assert_eq!(c.handle_key(key(KeyCode::Esc)), Outcome::Cancel);
    }

    #[test]
    fn ctrl_c_cancels() {
        let mut c = sample();
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(c.handle_key(ctrl_c), Outcome::Cancel);
    }
}
