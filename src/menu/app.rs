//! The palette's mutable state (`MenuApp`) and its pure key handler
//! (`handle_key`). The list is filtered by `query`; `selected` indexes into
//! the filtered view, never into `rows` directly. Navigation and filtering are
//! pure methods so they're unit-testable without a TUI.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::state::PanelSide;

use super::filter::filter_indices;
use super::model::{Choice, Targets, items};

/// Menu state. The list is filtered by `query`; `selected` indexes into the
/// filtered view, never into `rows` directly. Navigation and filtering are
/// pure methods so they're unit-testable without a TUI.
pub struct MenuApp {
    query: String,
    /// The ordered rows, built for the current panel side at open time.
    rows: Vec<(Choice, String)>,
    /// Indices into `rows` that match the current query, in menu order.
    filtered: Vec<usize>,
    /// Index into `filtered` of the highlighted row.
    selected: usize,
}

impl MenuApp {
    /// Open the palette for the given panel side (controls the layout-toggle
    /// row's label). `include_msg` controls whether the "Message brain" row
    /// is offered (hidden when the brain panel is already open). `targets`
    /// carries the highlighted entry's contextual row text: when a field is
    /// `Some`, the corresponding row appears — the "Create PDF" / "Open file"
    /// / "Open directory" rows lead the list, and "Delete" trails it.
    #[must_use]
    pub fn new(side: PanelSide, include_msg: bool, targets: &Targets) -> Self {
        let mut app = Self {
            query: String::new(),
            rows: items(side, include_msg, targets),
            filtered: Vec::new(),
            selected: 0,
        };
        app.refilter();
        app
    }

    fn refilter(&mut self) {
        self.filtered = filter_indices(&self.rows, &self.query);
        self.selected = 0;
    }

    const fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    /// The `Choice` under the cursor, or `None` when nothing matches.
    fn selected_choice(&self) -> Option<Choice> {
        self.filtered.get(self.selected).map(|&i| self.rows[i].0)
    }

    /// The current query text (for the rendered input line).
    pub(super) fn query(&self) -> &str {
        &self.query
    }

    /// The full ordered row list built at open time.
    pub(super) fn rows(&self) -> &[(Choice, String)] {
        &self.rows
    }

    /// Indices into `rows` that match the current query, in menu order.
    pub(super) fn filtered(&self) -> &[usize] {
        &self.filtered
    }

    /// Index into `filtered` of the highlighted row.
    pub(super) const fn selected(&self) -> usize {
        self.selected
    }
}

/// What a keypress asks the menu loop to do next. Split out as a pure
/// function (`handle_key`) so navigation is unit-testable without a TUI.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Keep looping.
    Continue,
    /// Confirm this choice.
    Confirm(Choice),
    /// Esc / Ctrl-c: close the palette with no choice.
    Cancel,
}

/// Pure key handling. Movement saturates at the ends; printable chars (and
/// Backspace / Ctrl-u / Ctrl-w) edit the query and refilter; Enter confirms
/// the highlighted row; Esc / Ctrl-c cancel.
pub fn handle_key(app: &mut MenuApp, k: KeyEvent) -> Step {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);

    match k.code {
        KeyCode::Esc => return Step::Cancel,
        KeyCode::Char('c') if ctrl => return Step::Cancel,
        KeyCode::Enter => {
            if let Some(choice) = app.selected_choice() {
                return Step::Confirm(choice);
            }
        }

        KeyCode::Up => app.move_up(),
        KeyCode::Char('p' | 'k') if ctrl => app.move_up(),
        KeyCode::Down => app.move_down(),
        KeyCode::Char('n' | 'j') if ctrl => app.move_down(),

        KeyCode::Backspace => {
            app.query.pop();
            app.refilter();
        }
        KeyCode::Char('u') if ctrl => {
            app.query.clear();
            app.refilter();
        }
        KeyCode::Char('w') if ctrl => {
            let cut = app
                .query
                .trim_end()
                .rfind(char::is_whitespace)
                .map_or(0, |i| i + 1);
            app.query.truncate(cut);
            app.refilter();
        }

        KeyCode::Char(c)
            if !k
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            app.query.push(c);
            app.refilter();
        }
        _ => {}
    }
    Step::Continue
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn new_app_selects_first_row_with_full_list() {
        let app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        assert_eq!(app.filtered.len(), app.rows.len());
        assert_eq!(app.selected, 0);
        assert_eq!(app.selected_choice(), Some(Choice::Msg));
    }

    #[test]
    fn down_moves_toward_the_end_and_saturates() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        app.move_down();
        assert_eq!(app.selected, 1);
        for _ in 0..50 {
            app.move_down();
        }
        assert_eq!(app.selected, app.filtered.len() - 1);
    }

    #[test]
    fn up_saturates_at_zero() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        app.move_up();
        assert_eq!(app.selected, 0);
        app.move_down();
        app.move_down();
        app.move_up();
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn filtering_resets_selection_and_tracks_filtered_view() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        app.move_down();
        app.move_down();
        for c in "tasks".chars() {
            handle_key(&mut app, key(KeyCode::Char(c)));
        }
        assert_eq!(app.filtered.len(), 1);
        assert_eq!(app.selected, 0);
        assert_eq!(app.selected_choice(), Some(Choice::OpenTasks));
    }

    #[test]
    fn typing_filters_and_backspace_restores() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        handle_key(&mut app, key(KeyCode::Char('7')));
        assert_eq!(app.query, "7");
        assert_eq!(app.selected_choice(), Some(Choice::GlobalSearch));
        handle_key(&mut app, key(KeyCode::Backspace));
        assert_eq!(app.query, "");
        assert_eq!(app.filtered.len(), app.rows.len());
    }

    #[test]
    fn ctrl_u_clears_the_query() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        for c in "search".chars() {
            handle_key(&mut app, key(KeyCode::Char(c)));
        }
        handle_key(&mut app, ctrl_key(KeyCode::Char('u')));
        assert_eq!(app.query, "");
        assert_eq!(app.filtered.len(), app.rows.len());
    }

    #[test]
    fn ctrl_w_deletes_the_last_word() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        for c in "search projects".chars() {
            handle_key(&mut app, key(KeyCode::Char(c)));
        }
        handle_key(&mut app, ctrl_key(KeyCode::Char('w')));
        assert_eq!(app.query, "search ");
    }

    #[test]
    fn ctrl_jk_mirror_arrows_over_the_filtered_list() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        handle_key(&mut app, ctrl_key(KeyCode::Char('j')));
        assert_eq!(app.selected, 1);
        handle_key(&mut app, ctrl_key(KeyCode::Char('k')));
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn plain_jk_are_query_input_not_navigation() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        handle_key(&mut app, key(KeyCode::Char('z')));
        assert_eq!(app.query, "z");
        assert!(app.filtered.is_empty());
        app.query.clear();
        app.refilter();
        handle_key(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.query, "j");
    }

    #[test]
    fn enter_confirms_the_highlighted_row() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        handle_key(&mut app, key(KeyCode::Down));
        // Row 1 (0-based) is Open tasks.
        assert_eq!(
            handle_key(&mut app, key(KeyCode::Enter)),
            Step::Confirm(Choice::OpenTasks)
        );
    }

    #[test]
    fn enter_with_no_matches_keeps_looping() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        for c in "zzz".chars() {
            handle_key(&mut app, key(KeyCode::Char(c)));
        }
        assert!(app.filtered.is_empty());
        assert_eq!(handle_key(&mut app, key(KeyCode::Enter)), Step::Continue);
    }

    #[test]
    fn esc_and_ctrl_c_cancel() {
        let mut app = MenuApp::new(PanelSide::Right, true, &Targets::default());
        assert_eq!(handle_key(&mut app, key(KeyCode::Esc)), Step::Cancel);
        assert_eq!(
            handle_key(&mut app, ctrl_key(KeyCode::Char('c'))),
            Step::Cancel
        );
    }
}
