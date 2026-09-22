//! The tree sub-view's key decisions.
//!
//! Pure: every arm either moves the widget's own state or names an effect for
//! the shell to run. Nothing here touches the filesystem or the terminal.
//! Movement produces no effect, because navigating is not a command.

use std::path::PathBuf;

use crossterm::event::KeyCode;

use crate::tui::state::BrainDirEffect;

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
) -> BrainDirEffect {
    match code {
        KeyCode::Char('c') if ctrl => BrainDirEffect::Quit,
        // Esc backs out of the tree rather than quitting the shell: the tree
        // is somewhere you drill into, so Esc is how you come back up.
        KeyCode::Esc => BrainDirEffect::BackToSearch,
        KeyCode::Enter if alt => BrainDirEffect::BackToSearch,
        KeyCode::Enter => match view.selected_path() {
            // The ../ row is navigation wearing an entry's clothes: it moves
            // the root rather than acting on a file.
            Some(path) if view.is_parent_row(&path) => BrainDirEffect::Reroot(path),
            Some(path) if ctrl => BrainDirEffect::Reveal(path),
            Some(path) => BrainDirEffect::Open(path),
            None => BrainDirEffect::None,
        },
        KeyCode::Char('p') if ctrl => BrainDirEffect::OpenPalette,
        KeyCode::Char('r') if ctrl => BrainDirEffect::Refresh,
        KeyCode::Char('g') if ctrl => {
            entry_selection(view).map_or(BrainDirEffect::None, BrainDirEffect::ConfirmPdf)
        }
        KeyCode::Char('d') if ctrl => {
            entry_selection(view).map_or(BrainDirEffect::None, BrainDirEffect::ConfirmDelete)
        }
        KeyCode::Char('e') if ctrl => BrainDirEffect::OpenExplorer,
        // The conventional dotfile key, free here because the tree has no
        // query line for a printable character to type into.
        KeyCode::Char('.') => BrainDirEffect::ToggleHiddenFiles,
        // `hjkl` are aliases for the arrows, merged into one arm per direction
        // so `Ctrl+J` / `Ctrl+K` land here too. `Ctrl+H` / `Ctrl+L` never do:
        // the main-view cycle consumes them upstream.
        KeyCode::Up | KeyCode::Char('k') => navigate(view, TreeMove::Up),
        KeyCode::Down | KeyCode::Char('j') => navigate(view, TreeMove::Down),
        KeyCode::Left | KeyCode::Char('h') => navigate(view, TreeMove::Collapse),
        KeyCode::Right | KeyCode::Char('l') => navigate(view, TreeMove::Expand),
        // Shifted, and about the level rather than the tree: the ends of the
        // selected node's own sibling list, where Home / End take the whole
        // visible tree.
        KeyCode::Char('H') => navigate(view, TreeMove::FirstSibling),
        KeyCode::Char('L') => navigate(view, TreeMove::LastSibling),
        KeyCode::Char(' ') => navigate(view, TreeMove::Toggle),
        KeyCode::PageUp => navigate(view, TreeMove::PageUp),
        KeyCode::PageDown => navigate(view, TreeMove::PageDown),
        KeyCode::Home => navigate(view, TreeMove::First),
        KeyCode::End => navigate(view, TreeMove::Last),
        _ => BrainDirEffect::None,
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
    FirstSibling,
    LastSibling,
}

/// Apply a movement and report that nothing else needs to happen.
///
/// No movement may leave the tree with nothing selected. `key_left` pops the
/// last identifier when there is no open node to close, and every vertical
/// move falls back to an empty identifier until the widget has recorded a
/// render. Either one erases the cursor, drops the highlight gutter (shifting
/// every row three columns), and leaves `Enter` inert until an arrow key
/// happens to recover.
fn navigate(view: &mut TreeView, movement: TreeMove) -> BrainDirEffect {
    let previous = view.state_mut().selected().to_vec();
    apply_move(view, movement);
    let state = view.state_mut();
    if state.selected().is_empty() && !previous.is_empty() {
        state.select(previous);
    }
    state.scroll_selected_into_view();
    BrainDirEffect::None
}

/// Run one movement. The sibling jumps read the built items rather than the
/// widget state, so they go through `TreeView` while the rest drive the widget
/// directly.
fn apply_move(view: &mut TreeView, movement: TreeMove) {
    match movement {
        TreeMove::Up => {
            view.state_mut().key_up();
        }
        TreeMove::Down => {
            view.state_mut().key_down();
        }
        TreeMove::Collapse => {
            view.state_mut().key_left();
        }
        TreeMove::Expand => {
            view.state_mut().key_right();
        }
        TreeMove::Toggle => {
            view.state_mut().toggle_selected();
        }
        TreeMove::PageUp => {
            for _ in 0..PAGE {
                view.state_mut().key_up();
            }
        }
        TreeMove::PageDown => {
            for _ in 0..PAGE {
                view.state_mut().key_down();
            }
        }
        TreeMove::First => {
            view.state_mut().select_first();
        }
        TreeMove::Last => {
            view.state_mut().select_last();
        }
        TreeMove::FirstSibling => view.select_first_sibling(),
        TreeMove::LastSibling => view.select_last_sibling(),
    }
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
    use ratatui::{Terminal, backend::TestBackend};
    use std::path::Path;

    fn entries() -> Vec<Entry> {
        vec![
            Entry {
                path: PathBuf::from("/brain/projects/atlas"),
                display: "~/brain/projects/atlas".to_owned(),
                bucket: Bucket::Projects,
                is_dir: true,
                is_hidden: false,
            },
            Entry {
                path: PathBuf::from("/brain/projects/atlas/plan.md"),
                display: "~/brain/projects/atlas/plan.md".to_owned(),
                bucket: Bucket::Projects,
                is_dir: false,
                is_hidden: false,
            },
        ]
    }

    fn view() -> TreeView {
        TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/atlas/plan.md"),
            false,
        )
    }

    /// Draw the panel once, so the widget has recorded the identifier list its
    /// vertical moves read. They are inert until it has.
    fn render_once(view: &mut TreeView) {
        let mut terminal = Terminal::new(TestBackend::new(48, 10)).expect("test terminal");
        terminal
            .draw(|frame| crate::tree::view::draw_into(frame, view, frame.area()))
            .expect("draw the tree panel");
    }

    #[test]
    fn enter_opens_the_selected_entry() {
        let mut view = view();
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Enter, false, false),
            BrainDirEffect::Open(PathBuf::from("/brain/projects/atlas/plan.md"))
        );
    }

    #[test]
    fn ctrl_enter_reveals_the_selected_entry() {
        let mut view = view();
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Enter, true, false),
            BrainDirEffect::Reveal(PathBuf::from("/brain/projects/atlas/plan.md"))
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
            BrainDirEffect::Reroot(PathBuf::from("/brain"))
        );
    }

    #[test]
    fn escape_and_alt_enter_both_return_to_search() {
        let mut view = view();
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Esc, false, false),
            BrainDirEffect::BackToSearch
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Enter, false, true),
            BrainDirEffect::BackToSearch
        );
    }

    #[test]
    fn ctrl_c_quits_the_shell_but_escape_does_not() {
        let mut view = view();
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('c'), true, false),
            BrainDirEffect::Quit
        );
        assert_ne!(
            handle_tree_input(&mut view, KeyCode::Esc, false, false),
            BrainDirEffect::Quit
        );
    }

    #[test]
    fn the_entry_commands_keep_the_keys_they_have_in_search() {
        let mut view = view();
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('g'), true, false),
            BrainDirEffect::ConfirmPdf(PathBuf::from("/brain/projects/atlas/plan.md"))
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('d'), true, false),
            BrainDirEffect::ConfirmDelete(PathBuf::from("/brain/projects/atlas/plan.md"))
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('r'), true, false),
            BrainDirEffect::Refresh
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('p'), true, false),
            BrainDirEffect::OpenPalette
        );
    }

    #[test]
    fn pdf_and_delete_do_nothing_on_the_parent_row() {
        let mut view = view();
        view.state_mut().select(vec![PathBuf::from("/brain")]);

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('g'), true, false),
            BrainDirEffect::None
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('d'), true, false),
            BrainDirEffect::None
        );
    }

    #[test]
    fn right_opens_the_selected_node_and_space_toggles_it() {
        // `key_right` and `toggle_selected` read only the selection, never the
        // identifier list from the last render, so the open set is observable
        // here without drawing anything.
        let mut view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/atlas"),
            false,
        );
        let atlas = vec![PathBuf::from("/brain/projects/atlas")];
        assert!(view.state.opened().is_empty(), "nothing starts open");

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Right, false, false),
            BrainDirEffect::None
        );
        assert!(
            view.state.opened().contains(&atlas),
            "\u{2192} opens the selected directory"
        );

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char(' '), false, false),
            BrainDirEffect::None
        );
        assert!(
            !view.state.opened().contains(&atlas),
            "Space closes it again"
        );

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char(' '), false, false),
            BrainDirEffect::None
        );
        assert!(view.state.opened().contains(&atlas), "and opens it back");
    }

    #[test]
    fn collapsing_a_top_level_row_keeps_it_selected() {
        // With nothing open, `key_left` has no node to close and falls through
        // to popping the last identifier, which on a depth-0 row leaves no
        // cursor at all: no highlight, every row shifted three columns, and
        // Enter inert until an arrow key recovers.
        let mut view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/atlas"),
            false,
        );
        assert!(view.state.opened().is_empty(), "nothing to collapse");

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Left, false, false),
            BrainDirEffect::None
        );

        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/atlas"))
        );
    }

    #[test]
    fn collapsing_an_open_node_still_closes_it() {
        let mut view = view();
        let atlas = vec![PathBuf::from("/brain/projects/atlas")];
        view.state.select(atlas.clone());
        assert!(view.state.opened().contains(&atlas));

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Left, false, false),
            BrainDirEffect::None
        );

        assert!(
            !view.state.opened().contains(&atlas),
            "\u{2190} still collapses an open node"
        );
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/atlas")),
            "and collapsing does not move the cursor"
        );
    }

    #[test]
    fn movement_keys_produce_no_effect_and_always_leave_a_cursor() {
        // Before a render these moves cannot be observed by where they land:
        // the widget has no identifier list to move through. What they must
        // never do is observable, and is the whole bug.
        let mut view = view();
        let selected = view.selected_path();

        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Home,
            KeyCode::End,
            // The vertical aliases are inert before a render for the same
            // reason the arrows are; `h` / `l` are not (they collapse and
            // expand), so they are exercised by the alias test instead.
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char('H'),
            KeyCode::Char('L'),
        ] {
            assert_eq!(
                handle_tree_input(&mut view, code, false, false),
                BrainDirEffect::None,
                "{code:?} is navigation, not a command"
            );
            assert_eq!(
                view.selected_path(),
                selected,
                "{code:?} must not leave the tree with nothing selected"
            );
        }
    }

    #[test]
    fn ctrl_j_and_ctrl_k_move_the_selection_like_the_search_view() {
        // Drawn once, the vertical moves really do step through the visible
        // rows: `../`, `atlas`, and the open directory's `plan.md`.
        let mut view = view();
        render_once(&mut view);
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/atlas/plan.md"))
        );

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('k'), true, false),
            BrainDirEffect::None
        );
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/atlas")),
            "Ctrl+K moves up one visible row"
        );

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('j'), true, false),
            BrainDirEffect::None
        );
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/atlas/plan.md")),
            "Ctrl+J moves back down"
        );
    }

    #[test]
    fn typing_an_unbound_character_does_nothing_because_the_tree_has_no_query() {
        let mut view = view();
        for character in ['a', 'Z', '/', '?'] {
            assert_eq!(
                handle_tree_input(&mut view, KeyCode::Char(character), false, false),
                BrainDirEffect::None
            );
        }
    }

    #[test]
    fn hjkl_alias_the_arrow_keys() {
        let mut view = view();
        render_once(&mut view);

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('k'), false, false),
            BrainDirEffect::None
        );
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/atlas")),
            "k moves up one visible row, like the up arrow"
        );

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('j'), false, false),
            BrainDirEffect::None
        );
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/atlas/plan.md")),
            "j moves back down"
        );

        // `l` / `h` open and close, which the open set shows with no render.
        let mut view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/atlas"),
            false,
        );
        let atlas = vec![PathBuf::from("/brain/projects/atlas")];

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('l'), false, false),
            BrainDirEffect::None
        );
        assert!(
            view.state.opened().contains(&atlas),
            "l expands the selected directory, like the right arrow"
        );

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('h'), false, false),
            BrainDirEffect::None
        );
        assert!(
            !view.state.opened().contains(&atlas),
            "h collapses it again, like the left arrow"
        );
    }

    #[test]
    fn shifted_h_and_l_jump_to_the_first_and_last_sibling() {
        // The level's own ends, not the tree's: Home / End still do that.
        let entries = vec![
            Entry {
                path: PathBuf::from("/brain/projects/atlas"),
                display: "~/brain/projects/atlas".to_owned(),
                bucket: Bucket::Projects,
                is_dir: true,
                is_hidden: false,
            },
            Entry {
                path: PathBuf::from("/brain/projects/atlas/a.md"),
                display: "~/brain/projects/atlas/a.md".to_owned(),
                bucket: Bucket::Projects,
                is_dir: false,
                is_hidden: false,
            },
            Entry {
                path: PathBuf::from("/brain/projects/atlas/b.md"),
                display: "~/brain/projects/atlas/b.md".to_owned(),
                bucket: Bucket::Projects,
                is_dir: false,
                is_hidden: false,
            },
        ];
        let mut view = TreeView::explore(
            &entries,
            Path::new("/brain"),
            Path::new("/brain/projects/atlas/b.md"),
            false,
        );

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('H'), false, false),
            BrainDirEffect::None
        );
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/atlas/a.md"))
        );

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('L'), false, false),
            BrainDirEffect::None
        );
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/atlas/b.md")),
            "and never leaves the directory it started in"
        );
    }

    #[test]
    fn dot_toggles_hidden_files_and_ctrl_e_opens_the_explorer() {
        // Both need a fresh walk, so the tree names an effect rather than
        // doing it: the entries it holds have no dotted names in them at all.
        let mut view = view();

        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('.'), false, false),
            BrainDirEffect::ToggleHiddenFiles
        );
        assert_eq!(
            handle_tree_input(&mut view, KeyCode::Char('e'), true, false),
            BrainDirEffect::OpenExplorer
        );
    }
}
