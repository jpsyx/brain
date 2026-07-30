//! The app-level "main view" axis and its view-switching key decisions.
//!
//! The merged `brain` shell has three **main views** — the tasks view, the
//! brain-directory (fuzzy search) view, and the log view — plus one app-level **brain panel**
//! (the `claude` PTY) that is independent of which main view is showing. This
//! module owns the pure logic for that axis: the [`MainView`] enum, cycling
//! ([`MainView::step`]), and the pure key-classifiers that map a keystroke to
//! a view switch. The `App` applies the result; nothing here mutates state.

use crossterm::event::KeyCode;

/// Which full-screen surface is currently showing.
///
/// The main views sit next to (or instead of) the brain panel. The brain
/// panel itself is *not* a main view — it is app-level and persists across a
/// `MainView` switch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MainView {
    /// The tasks view (task management, agenda, triage) — the startup default.
    Tasks,
    /// The brain-directory fuzzy-search view (formerly bare `brain`).
    BrainSearch,
    /// The scrollable diagnostic log view.
    Logs,
}

/// A horizontal cycle direction.
///
/// With only two main views today, `Left` and `Right` land on the same place,
/// but the distinction is kept so a future third view makes them differ.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Left,
    Right,
}

impl MainView {
    /// The main views in cycle order (left-to-right). `Ctrl+L` advances
    /// forward through this, `Ctrl+H` backward; both wrap.
    pub const CYCLE: [Self; 3] = [Self::Tasks, Self::BrainSearch, Self::Logs];

    /// The next main view one step in `dir`, wrapping around [`Self::CYCLE`].
    #[must_use]
    pub fn step(self, dir: Dir) -> Self {
        let len = Self::CYCLE.len();
        let idx = Self::CYCLE.iter().position(|v| *v == self).unwrap_or(0);
        let next = match dir {
            Dir::Right => (idx + 1) % len,
            Dir::Left => (idx + len - 1) % len,
        };
        Self::CYCLE[next]
    }
}

/// Whether a keystroke cycles the main view, and in which direction.
///
/// `Ctrl+H` → [`Dir::Left`], `Ctrl+L` → [`Dir::Right`]. `None` for anything
/// else. Matches both cases of the letter since terminals differ on whether
/// they report the shifted glyph. Note: `Ctrl+H` is `0x08` (BS) and collapses
/// to a bare `Backspace` on terminals without the kitty keyboard protocol;
/// there the palette's "cycle view" rows are the fallback.
#[must_use]
pub const fn ctrl_cycles_view(code: KeyCode, ctrl: bool) -> Option<Dir> {
    if !ctrl {
        return None;
    }
    match code {
        KeyCode::Char('h' | 'H') => Some(Dir::Left),
        KeyCode::Char('l' | 'L') => Some(Dir::Right),
        _ => None,
    }
}

/// Whether a keystroke jumps directly to a specific main view.
///
/// `Ctrl+T` → [`MainView::Tasks`], `Ctrl+B` → [`MainView::BrainSearch`].
/// `None` for anything else.
#[must_use]
pub const fn ctrl_jumps_view(code: KeyCode, ctrl: bool) -> Option<MainView> {
    if !ctrl {
        return None;
    }
    match code {
        KeyCode::Char('t' | 'T') => Some(MainView::Tasks),
        KeyCode::Char('b' | 'B') => Some(MainView::BrainSearch),
        _ => None,
    }
}

/// Whether a keystroke opens the shortcuts help modal.
///
/// Bound to `Alt+S` (not a bare key), so that in the always-filtering
/// brain-search view a literal `s` still types into the fuzzy query. `Alt+S`
/// arrives as a distinct Meta sequence on every terminal, so it never depends
/// on the kitty protocol. Matches both cases since terminals differ on the
/// reported glyph.
#[must_use]
pub const fn alt_opens_help(code: KeyCode, alt: bool) -> bool {
    alt && matches!(code, KeyCode::Char('s' | 'S'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_cycles_and_wraps_both_directions() {
        assert_eq!(MainView::Tasks.step(Dir::Right), MainView::BrainSearch);
        assert_eq!(MainView::BrainSearch.step(Dir::Right), MainView::Logs);
        assert_eq!(MainView::Logs.step(Dir::Right), MainView::Tasks);
        assert_eq!(MainView::Tasks.step(Dir::Left), MainView::Logs);
        assert_eq!(MainView::BrainSearch.step(Dir::Left), MainView::Tasks);
    }

    #[test]
    fn ctrl_h_cycles_left_and_ctrl_l_cycles_right() {
        assert_eq!(ctrl_cycles_view(KeyCode::Char('h'), true), Some(Dir::Left));
        assert_eq!(ctrl_cycles_view(KeyCode::Char('H'), true), Some(Dir::Left));
        assert_eq!(ctrl_cycles_view(KeyCode::Char('l'), true), Some(Dir::Right));
        assert_eq!(ctrl_cycles_view(KeyCode::Char('L'), true), Some(Dir::Right));
    }

    #[test]
    fn cycle_view_requires_ctrl_and_ignores_other_keys() {
        assert_eq!(ctrl_cycles_view(KeyCode::Char('h'), false), None);
        assert_eq!(ctrl_cycles_view(KeyCode::Char('l'), false), None);
        assert_eq!(ctrl_cycles_view(KeyCode::Char('j'), true), None);
    }

    #[test]
    fn ctrl_t_jumps_to_tasks_and_ctrl_b_jumps_to_brain() {
        assert_eq!(
            ctrl_jumps_view(KeyCode::Char('t'), true),
            Some(MainView::Tasks)
        );
        assert_eq!(
            ctrl_jumps_view(KeyCode::Char('T'), true),
            Some(MainView::Tasks)
        );
        assert_eq!(
            ctrl_jumps_view(KeyCode::Char('b'), true),
            Some(MainView::BrainSearch)
        );
        assert_eq!(
            ctrl_jumps_view(KeyCode::Char('B'), true),
            Some(MainView::BrainSearch)
        );
    }

    #[test]
    fn jump_view_requires_ctrl_and_ignores_other_keys() {
        assert_eq!(ctrl_jumps_view(KeyCode::Char('t'), false), None);
        assert_eq!(ctrl_jumps_view(KeyCode::Char('b'), false), None);
        assert_eq!(ctrl_jumps_view(KeyCode::Char('x'), true), None);
    }

    #[test]
    fn alt_s_opens_help_bare_s_does_not() {
        assert!(alt_opens_help(KeyCode::Char('s'), true));
        assert!(alt_opens_help(KeyCode::Char('S'), true));
        assert!(!alt_opens_help(KeyCode::Char('s'), false));
        assert!(!alt_opens_help(KeyCode::Char('?'), true));
        assert!(!alt_opens_help(KeyCode::Char('a'), true));
    }
}
