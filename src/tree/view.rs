//! Rendering the tree sub-view.

use ratatui::{Frame, layout::Rect};
use tui_tree_widget::Tree;

use super::TreeView;

/// Draw the tree into `area`.
///
/// The items and the widget state are borrowed as separate fields rather than
/// through `items()` / `state_mut()`, because the widget holds the item slice
/// while the state is borrowed mutably.
// Chrome lands with the renderer task.
pub(crate) fn draw_into(f: &mut Frame, view: &mut TreeView, area: Rect) {
    // `Tree::new` only errors on duplicate identifiers, which distinct
    // absolute paths cannot produce.
    if let Ok(tree) = Tree::new(&view.items) {
        f.render_stateful_widget(tree, area, &mut view.state);
    }
}
