//! Finding the ends of a node's own sibling list.
//!
//! Pure over the built items rather than over the widget's `TreeState`, whose
//! own navigation reads the identifier list it recorded on its last render and
//! is therefore inert before the first draw.

use std::path::{Path, PathBuf};

use tui_tree_widget::TreeItem;

/// The first and last sibling of the node `selected` addresses, as full
/// identifier paths.
///
/// Siblings are the children of the selected node's parent — the rows at its
/// own depth, under its own directory — so `H` / `L` stay inside the level the
/// cursor is on instead of jumping to the ends of the whole visible tree the
/// way `Home` / `End` do. `None` when the selection addresses nothing: it is
/// empty, or one of its steps names a node the items do not hold.
pub(crate) fn sibling_bounds(
    items: &[TreeItem<'static, PathBuf>],
    selected: &[PathBuf],
) -> Option<(Vec<PathBuf>, Vec<PathBuf>)> {
    let (_, parents) = selected.split_last()?;
    let mut siblings = items;
    for step in parents {
        siblings = children_of(siblings, step)?;
    }
    let first = siblings.first()?.identifier().clone();
    let last = siblings.last()?.identifier().clone();
    Some((addressed(parents, first), addressed(parents, last)))
}

/// The rows under the item `identifier` names, or `None` when this level does
/// not hold it.
fn children_of<'a>(
    items: &'a [TreeItem<'static, PathBuf>],
    identifier: &Path,
) -> Option<&'a [TreeItem<'static, PathBuf>]> {
    items
        .iter()
        .find(|item| item.identifier().as_path() == identifier)
        .map(TreeItem::children)
}

/// One sibling as the widget addresses it: its parents' identifiers, then its
/// own.
fn addressed(parents: &[PathBuf], sibling: PathBuf) -> Vec<PathBuf> {
    let mut path = parents.to_vec();
    path.push(sibling);
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    use ratatui::text::Line;

    fn leaf(path: &str) -> TreeItem<'static, PathBuf> {
        TreeItem::new_leaf(PathBuf::from(path), Line::raw(path.to_owned()))
    }

    fn node(path: &str, children: Vec<TreeItem<'static, PathBuf>>) -> TreeItem<'static, PathBuf> {
        TreeItem::new(PathBuf::from(path), Line::raw(path.to_owned()), children)
            .expect("distinct identifiers")
    }

    /// The shape a scoped tree has: the `../` row, then a directory with three
    /// children, then a loose file.
    fn items() -> Vec<TreeItem<'static, PathBuf>> {
        vec![
            leaf("/brain"),
            node(
                "/brain/projects/atlas",
                vec![
                    leaf("/brain/projects/atlas/a.md"),
                    leaf("/brain/projects/atlas/b.md"),
                    leaf("/brain/projects/atlas/c.md"),
                ],
            ),
            leaf("/brain/projects/loose.md"),
        ]
    }

    fn path(parts: &[&str]) -> Vec<PathBuf> {
        parts.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn a_nested_child_finds_the_ends_of_its_own_directory() {
        let bounds = sibling_bounds(
            &items(),
            &path(&["/brain/projects/atlas", "/brain/projects/atlas/b.md"]),
        );

        assert_eq!(
            bounds,
            Some((
                path(&["/brain/projects/atlas", "/brain/projects/atlas/a.md"]),
                path(&["/brain/projects/atlas", "/brain/projects/atlas/c.md"]),
            )),
            "both ends stay inside atlas/ rather than reaching the whole tree"
        );
    }

    #[test]
    fn a_top_level_row_counts_the_parent_row_as_its_first_sibling() {
        // `../` really is the first row of that list: it is navigation, but it
        // is a row, and jumping to the top of the level has to land on it.
        let bounds = sibling_bounds(&items(), &path(&["/brain/projects/loose.md"]));

        assert_eq!(
            bounds,
            Some((path(&["/brain"]), path(&["/brain/projects/loose.md"])))
        );
    }

    #[test]
    fn a_selection_that_addresses_nothing_has_no_siblings() {
        assert_eq!(sibling_bounds(&items(), &[]), None);
        assert_eq!(
            sibling_bounds(&items(), &path(&["/brain/projects/gone", "/x"])),
            None,
            "a step the items do not hold cannot name a level"
        );
        assert_eq!(sibling_bounds(&[], &path(&["/brain"])), None);
    }

    #[test]
    fn an_only_child_is_both_the_first_and_the_last_sibling() {
        let items = vec![node(
            "/brain/projects/atlas",
            vec![leaf("/brain/projects/atlas/only.md")],
        )];
        let selected = path(&["/brain/projects/atlas", "/brain/projects/atlas/only.md"]);

        let (first, last) = sibling_bounds(&items, &selected).expect("a level with one row");

        assert_eq!(first, selected);
        assert_eq!(first, last);
    }
}
