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

    // `Tree::new` only errors on duplicate identifiers, which distinct
    // absolute paths cannot produce. Borrowing `view.items`/`view.state` as
    // separate fields (rather than through the `items()`/`state_mut()`
    // methods) keeps the tree's borrow of the item slice disjoint from the
    // mutable borrow of the widget state below.
    if let Ok(tree) = Tree::new(&view.items) {
        let tree = tree
            .highlight_style(render::selected_row_style())
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
}
