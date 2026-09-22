//! Rendering the tree panel: header / separator / tree / footer, in the same
//! bordered sub-rect the search panel uses so the two sub-views are visually
//! interchangeable.

use std::path::Path;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Paragraph,
};
use tui_tree_widget::Tree;

use crate::render;

use super::TreeView;

/// Draw the tree into `area`.
///
/// The items and the widget state are borrowed as separate fields rather than
/// through `items()` / `state_mut()`, because the widget holds the item slice
/// while the state is borrowed mutably.
pub(crate) fn draw_into(f: &mut Frame, view: &mut TreeView, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Length(1), // separator
            Constraint::Min(1),    // tree
            Constraint::Length(1), // footer
        ])
        .split(area);

    let header = header_text(view.root(), view.brain_root());
    f.render_widget(
        Paragraph::new(render::tree_header_line(&header, view.items().len())),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(render::separator_line(area.width as usize)),
        chunks[1],
    );

    // Read before the field borrows below, because it needs the whole view.
    // One `Style` per frame, computed from the selected row's kind: no item
    // rebuild and nothing allocated to make the cursor's row its own colour.
    let highlight = view.selected_row_style();

    // `Tree::new` only errors on duplicate identifiers, which distinct
    // absolute paths cannot produce. Borrowing `view.items`/`view.state` as
    // separate fields (rather than through the `items()`/`state_mut()`
    // methods) keeps the tree's borrow of the item slice disjoint from the
    // mutable borrow of the widget state below.
    if let Ok(tree) = Tree::new(&view.items) {
        let tree = tree
            .highlight_style(highlight)
            .highlight_symbol(" ❯ ")
            .node_closed_symbol("▸ ")
            .node_open_symbol("▾ ")
            .node_no_children_symbol("  ");
        f.render_stateful_widget(tree, chunks[2], &mut view.state);
    }

    f.render_widget(Paragraph::new(render::tree_footer_line()), chunks[3]);
}

/// What the header says the tree is showing: the sub-view name and the root
/// as a brain-relative path, or `all` at the brain root itself.
fn header_text(root: &Path, brain_root: &Path) -> String {
    let relative = root
        .strip_prefix(brain_root)
        .ok()
        .map(|rel| rel.display().to_string())
        .filter(|rel| !rel.is_empty())
        .unwrap_or_else(|| "all".to_owned());
    format!("tree · {relative}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{Bucket, Entry};
    use crate::tree::kind::FileKind;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::{Buffer, Cell},
        style::Modifier,
    };
    use std::path::PathBuf;

    /// `is_hidden` is derived from the name, exactly as `entry::collect`
    /// derives it, so a dotted fixture is hidden without a third argument.
    fn entry(path: &str, is_dir: bool) -> Entry {
        let path = PathBuf::from(path);
        let display = path.display().to_string();
        let is_hidden = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'));
        Entry {
            path,
            display,
            bucket: Bucket::Projects,
            is_dir,
            is_hidden,
        }
    }

    /// Draw the panel and hand back the cells, so a test can read the style a
    /// row was painted with and not only its text.
    fn rendered_buffer(view: &mut TreeView, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
        terminal
            .draw(|frame| draw_into(frame, view, frame.area()))
            .expect("draw the tree panel");
        terminal.backend().buffer().clone()
    }

    /// Render the panel and return its rows as plain strings, so the test
    /// asserts on what a reader would actually see.
    fn rendered_rows(view: &mut TreeView, width: u16, height: u16) -> Vec<String> {
        let buffer = rendered_buffer(view, width, height);
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    /// The first cell of `needle` on screen. Matching cell-by-cell rather than
    /// through a joined string keeps the column honest whatever the glyphs are.
    fn cell_of(buffer: &Buffer, width: u16, height: u16, needle: &str) -> Cell {
        let wanted: Vec<String> = needle.chars().map(|c| c.to_string()).collect();
        for y in 0..height {
            let row: Vec<String> = (0..width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect();
            if let Some(start) = row
                .windows(wanted.len())
                .position(|window| window == wanted.as_slice())
            {
                let column = u16::try_from(start).expect("a column inside the test terminal");
                return buffer[(column, y)].clone();
            }
        }
        panic!("{needle:?} is not on screen");
    }

    #[test]
    fn the_panel_draws_a_header_the_tree_and_a_footer() {
        let entries = vec![
            entry("/brain/projects/atlas", true),
            entry("/brain/projects/atlas/plan.md", false),
            entry("/brain/projects/loose.md", false),
        ];
        let mut view = TreeView::explore(
            &entries,
            Path::new("/brain"),
            Path::new("/brain/projects/atlas/plan.md"),
            false,
        );

        let rows = rendered_rows(&mut view, 48, 10);
        let screen = rows.join("\n");

        assert!(
            rows[0].contains("tree \u{b7} projects"),
            "header: {:?}",
            rows[0]
        );
        assert!(rows[0].contains("items"), "header count: {:?}", rows[0]);
        // The ../ row, the expanded directory, its child, and the loose file.
        assert!(screen.contains("../"), "{screen}");
        assert!(screen.contains("atlas"), "{screen}");
        assert!(screen.contains("plan.md"), "{screen}");
        assert!(screen.contains("loose.md"), "{screen}");
        assert!(rows[9].contains("open"), "footer: {:?}", rows[9]);
    }

    #[test]
    fn a_collapsed_directory_renders_closed_and_hides_its_children() {
        // Nothing is opened, so the child must not be on screen: that is what
        // makes the tree a tree rather than the flat search list.
        let entries = vec![
            entry("/brain/projects/atlas", true),
            entry("/brain/projects/atlas/plan.md", false),
        ];
        let mut view = TreeView::empty(Path::new("/brain"));
        view.rebuild(&entries, Path::new("/brain/projects"));

        let screen = rendered_rows(&mut view, 48, 10).join("\n");

        assert!(screen.contains("\u{25b8} atlas"), "closed marker: {screen}");
        assert!(
            !screen.contains("plan.md"),
            "child must stay hidden: {screen}"
        );
    }

    #[test]
    fn the_header_names_the_sub_view_and_the_current_root() {
        assert_eq!(
            header_text(Path::new("/brain/projects"), Path::new("/brain")),
            "tree · projects"
        );
    }

    #[test]
    fn at_the_brain_root_the_header_says_so_rather_than_showing_an_empty_path() {
        assert_eq!(
            header_text(Path::new("/brain"), Path::new("/brain")),
            "tree · all"
        );
    }

    #[test]
    fn a_nested_root_shows_its_full_brain_relative_path() {
        assert_eq!(
            header_text(Path::new("/brain/projects/atlas"), Path::new("/brain")),
            "tree · projects/atlas"
        );
    }

    #[test]
    fn a_root_outside_the_brain_falls_back_rather_than_rendering_nothing() {
        assert_eq!(
            header_text(Path::new("/etc"), Path::new("/brain")),
            "tree · all"
        );
    }

    #[test]
    fn every_row_is_coloured_by_what_it_is() {
        let entries = vec![
            entry("/brain/projects/atlas", true),
            entry("/brain/projects/plan.md", false),
            entry("/brain/projects/data.json", false),
        ];
        let mut view = TreeView::empty(Path::new("/brain"));
        view.rebuild(&entries, Path::new("/brain/projects"));

        let buffer = rendered_buffer(&mut view, 48, 10);

        assert_eq!(
            cell_of(&buffer, 48, 10, "atlas/").fg,
            render::ACCENT_CYAN,
            "a directory reads as one by its slash and its hue"
        );
        assert_eq!(cell_of(&buffer, 48, 10, "plan.md").fg, render::TEXT_PRIMARY);
        assert_eq!(
            cell_of(&buffer, 48, 10, "data.json").fg,
            render::ACCENT_YELLOW
        );
        assert_eq!(
            cell_of(&buffer, 48, 10, "../").fg,
            render::TEXT_VERY_DIM,
            "the ../ row is navigation, not content"
        );
    }

    #[test]
    fn a_hidden_row_is_dimmed_but_keeps_its_kind_colour() {
        // A hidden row is only on screen once it has been asked for, so the
        // colour test has to ask: the dimming is what it looks like *then*.
        let entries = vec![entry("/brain/projects/.config.json", false)];
        let mut view = TreeView::empty(Path::new("/brain"));
        view.set_show_hidden(true);
        view.rebuild(&entries, Path::new("/brain/projects"));

        let buffer = rendered_buffer(&mut view, 48, 10);
        let cell = cell_of(&buffer, 48, 10, ".config.json");

        assert_eq!(
            cell.fg,
            render::ACCENT_YELLOW,
            "the hue still says what the thing is"
        );
        assert!(
            cell.modifier.contains(Modifier::DIM),
            "the dimming says it is normally out of sight"
        );
    }

    /// The perceived lightness of a painted cell's foreground.
    fn lightness_of(cell: &Cell) -> f64 {
        render::oklab::to_oklab(cell.fg)
            .expect("the tree palette is all RGB")
            .lightness
    }

    /// Two directories and two notes, so each kind has an unselected row of
    /// the same kind to be compared against.
    fn pairs() -> Vec<Entry> {
        vec![
            entry("/brain/projects/atlas", true),
            entry("/brain/projects/beacon", true),
            entry("/brain/projects/plan.md", false),
            entry("/brain/projects/second.md", false),
        ]
    }

    #[test]
    fn the_selected_row_is_painted_a_brighter_version_of_its_kind_colour() {
        let entries = pairs();
        let mut view = TreeView::empty(Path::new("/brain"));
        view.rebuild(&entries, Path::new("/brain/projects"));
        view.state_mut()
            .select(vec![PathBuf::from("/brain/projects/plan.md")]);

        let buffer = rendered_buffer(&mut view, 48, 10);
        let selected = cell_of(&buffer, 48, 10, "plan.md");
        let unselected = cell_of(&buffer, 48, 10, "second.md");

        assert_eq!(
            unselected.fg,
            render::TEXT_PRIMARY,
            "an unselected note keeps the palette's own note colour"
        );
        assert!(
            lightness_of(&selected) > lightness_of(&unselected),
            "the cursor's row must be visibly lighter: {:?} vs {:?}",
            selected.fg,
            unselected.fg
        );
        assert_eq!(
            selected.fg,
            render::tree_row_selected_style(FileKind::Note, false)
                .fg
                .expect("the selected style names a foreground"),
            "the lift is the one the palette defines, not a second opinion"
        );
    }

    #[test]
    fn a_selected_directory_is_brighter_cyan_than_an_unselected_one() {
        // The case that motivated the change: a directory's base style is
        // already bold, so bolding the cursor's row said nothing. Only the
        // colour can.
        let entries = pairs();
        let mut view = TreeView::empty(Path::new("/brain"));
        view.rebuild(&entries, Path::new("/brain/projects"));
        view.state_mut()
            .select(vec![PathBuf::from("/brain/projects/atlas")]);

        let buffer = rendered_buffer(&mut view, 48, 10);
        let selected = cell_of(&buffer, 48, 10, "atlas/");
        let unselected = cell_of(&buffer, 48, 10, "beacon/");

        assert_eq!(unselected.fg, render::ACCENT_CYAN);
        assert_ne!(
            selected.fg,
            render::ACCENT_CYAN,
            "a highlighted directory that is still plain cyan is the bug"
        );
        assert!(
            lightness_of(&selected) > lightness_of(&unselected),
            "the cursor's directory must be lighter: {:?} vs {:?}",
            selected.fg,
            unselected.fg
        );
        assert!(
            selected.modifier.contains(Modifier::BOLD),
            "the weight the row already had is not taken away"
        );
    }

    #[test]
    fn only_the_selected_row_takes_the_selection_background() {
        let entries = pairs();
        let mut view = TreeView::empty(Path::new("/brain"));
        view.rebuild(&entries, Path::new("/brain/projects"));
        view.state_mut()
            .select(vec![PathBuf::from("/brain/projects/atlas")]);

        let buffer = rendered_buffer(&mut view, 48, 10);

        assert_eq!(cell_of(&buffer, 48, 10, "atlas/").bg, render::SELECTED_BG);
        assert_ne!(cell_of(&buffer, 48, 10, "beacon/").bg, render::SELECTED_BG);
        assert_ne!(cell_of(&buffer, 48, 10, "plan.md").bg, render::SELECTED_BG);
    }
}
