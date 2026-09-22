//! The tree sub-view's key decisions.
//!
//! Pure: every arm either moves the widget's own state or names an effect for
//! the shell to run. Nothing here touches the filesystem or the terminal.
//! Movement produces no effect, because navigating is not a command.

use std::path::PathBuf;

use crossterm::event::KeyCode;

use crate::tui::state::SearchEffect;

use super::TreeView;

/// How far a page key moves. Matches the search picker's `PAGE_SIZE` so the
/// two sub-views scroll at the same rate.
const PAGE: usize = 10;

/// Route one keystroke in the tree sub-view.
///
/// Everything that *acts* names the same effect the search sub-view would, so
/// both sub-views run through one applier.
pub(crate) fn handle_tree_input(
    view: &mut TreeView,
    code: KeyCode,
    ctrl: bool,
    alt: bool,
) -> SearchEffect {
    match code {
        KeyCode::Char('c') if ctrl => SearchEffect::Quit,
        // Esc backs out of the tree rather than quitting the shell: the tree
        // is somewhere you drill into, so Esc is how you come back up.
        KeyCode::Esc => SearchEffect::BackToSearch,
        KeyCode::Enter if alt => SearchEffect::BackToSearch,
        KeyCode::Enter => match view.selected_path() {
            // The ../ row is navigation wearing an entry's clothes: it moves
            // the root rather than acting on a file.
            Some(path) if view.is_parent_row(&path) => SearchEffect::Reroot(path),
            Some(path) if ctrl => SearchEffect::Reveal(path),
            Some(path) => SearchEffect::Open(path),
            None => SearchEffect::None,
        },
        KeyCode::Char('p') if ctrl => SearchEffect::OpenPalette,
        KeyCode::Char('r') if ctrl => SearchEffect::Refresh,
        KeyCode::Char('g') if ctrl => {
            entry_selection(view).map_or(SearchEffect::None, SearchEffect::ConfirmPdf)
        }
        KeyCode::Char('d') if ctrl => {
            entry_selection(view).map_or(SearchEffect::None, SearchEffect::ConfirmDelete)
        }
        KeyCode::Up => navigate(view, TreeMove::Up),
        KeyCode::Down => navigate(view, TreeMove::Down),
        KeyCode::Char('k') if ctrl => navigate(view, TreeMove::Up),
        KeyCode::Char('j') if ctrl => navigate(view, TreeMove::Down),
        KeyCode::Left => navigate(view, TreeMove::Collapse),
        KeyCode::Right => navigate(view, TreeMove::Expand),
        KeyCode::Char(' ') => navigate(view, TreeMove::Toggle),
        KeyCode::PageUp => navigate(view, TreeMove::PageUp),
        KeyCode::PageDown => navigate(view, TreeMove::PageDown),
        KeyCode::Home => navigate(view, TreeMove::First),
        KeyCode::End => navigate(view, TreeMove::Last),
        _ => SearchEffect::None,
    }
}

#[derive(Clone, Copy)]
enum TreeMove {
    Up,
    Down,
    Collapse,
    Expand,
    Toggle,
    PageUp,
    PageDown,
    First,
    Last,
}

/// Apply a movement and report that nothing else needs to happen.
fn navigate(view: &mut TreeView, movement: TreeMove) -> SearchEffect {
    let state = view.state_mut();
    match movement {
        TreeMove::Up => {
            state.key_up();
        }
        TreeMove::Down => {
            state.key_down();
        }
        TreeMove::Collapse => {
            state.key_left();
        }
        TreeMove::Expand => {
            state.key_right();
        }
        TreeMove::Toggle => {
            state.toggle_selected();
        }
        TreeMove::PageUp => {
            for _ in 0..PAGE {
                state.key_up();
            }
        }
        TreeMove::PageDown => {
            for _ in 0..PAGE {
                state.key_down();
            }
        }
        TreeMove::First => {
            state.select_first();
        }
        TreeMove::Last => {
            state.select_last();
        }
    }
    state.scroll_selected_into_view();
    SearchEffect::None
}

/// The selected path when it is a real entry, filtering out the `../` row so
/// an entry command never targets it.
fn entry_selection(view: &TreeView) -> Option<PathBuf> {
    let path = view.selected_path()?;
    (!view.is_parent_row(&path)).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{Bucket, Entry};
    use std::path::Path;

    fn entries() -> Vec<Entry> {
        vec![
            Entry {
                path: PathBuf::from("/brain/projects/atlas"),
                display: "~/brain/projects/atlas".to_owned(),
                bucket: Bucket::Projects,
                is_dir: true,
            },
            Entry {
                path: PathBuf::from("/brain/projects/atlas/plan.md"),
                display: "~/brain/projects/atlas/plan.md".to_owned(),
                bucket: Bucket::Projects,
                is_dir: false,
            },
        ]
    }

    fn view() -> TreeView {
        TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/atlas/plan.md"),
        )
    }

    #[test]
    fn enter_opens_the_selected_entry() {
        let mut view = view();
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Enter, false, false),
            SearchEffect::Open(PathBuf::from("/brain/projects/atlas/plan.md"))
        );
    }

    #[test]
    fn ctrl_enter_reveals_the_selected_entry() {
        let mut view = view();
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Enter, true, false),
            SearchEffect::Reveal(PathBuf::from("/brain/projects/atlas/plan.md"))
        );
    }

    #[test]
    fn enter_on_the_parent_row_re_roots_instead_of_opening() {
        // The one case where Enter does not act on an entry: the synthetic ../
        // row is navigation, not a file.
        let mut view = view();
        view.state_mut().select(vec![PathBuf::from("/brain")]);

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Enter, false, false),
            SearchEffect::Reroot(PathBuf::from("/brain"))
        );
    }

    #[test]
    fn escape_and_alt_enter_both_return_to_search() {
        let mut view = view();
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Esc, false, false),
            SearchEffect::BackToSearch
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Enter, false, true),
            SearchEffect::BackToSearch
        );
    }

    #[test]
    fn ctrl_c_quits_the_shell_but_escape_does_not() {
        let mut view = view();
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('c'), true, false),
            SearchEffect::Quit
        );
        assert_ne!(
            handle_tree_input(&mut view, KeyCode::Esc, false, false),
            SearchEffect::Quit
        );
    }

    #[test]
    fn the_entry_commands_keep_the_keys_they_have_in_search() {
        let mut view = view();
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('g'), true, false),
            SearchEffect::ConfirmPdf(PathBuf::from("/brain/projects/atlas/plan.md"))
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('d'), true, false),
            SearchEffect::ConfirmDelete(PathBuf::from("/brain/projects/atlas/plan.md"))
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('r'), true, false),
            SearchEffect::Refresh
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('p'), true, false),
            SearchEffect::OpenPalette
        );
    }

    #[test]
    fn pdf_and_delete_do_nothing_on_the_parent_row() {
        let mut view = view();
        view.state_mut().select(vec![PathBuf::from("/brain")]);

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('g'), true, false),
            SearchEffect::None
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('d'), true, false),
            SearchEffect::None
        );
    }

    #[test]
    fn arrows_move_and_expand_without_producing_an_effect() {
        let mut view = view();
        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Char(' '),
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Home,
            KeyCode::End,
        ] {
            assert_eq!(
                handle_tree_input(&mut view, code, false, false),
                SearchEffect::None,
                "{code:?} is navigation, not a command"
            );
        }
    }

    #[test]
    fn ctrl_j_and_ctrl_k_move_the_selection_like_the_search_view() {
        let mut view = view();
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('k'), true, false),
            SearchEffect::None
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('j'), true, false),
            SearchEffect::None
        );
    }

    #[test]
    fn typing_does_nothing_because_the_tree_has_no_query() {
        let mut view = view();
        for character in ['a', 'Z', '/', '?'] {
            assert_eq!(
                handle_tree_input(&mut view, KeyCode::Char(character), false, false),
                SearchEffect::None
            );
        }
    }
}
