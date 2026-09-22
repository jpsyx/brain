//! Turning the flat entry list into the nested tree the widget renders.
//!
//! Everything here is pure: it reads `Entry::is_dir` rather than the
//! filesystem, so the whole shape of the tree is testable without a temp dir.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use tui_tree_widget::TreeItem;

use crate::entry::Entry;

use super::root::{ascend, shows_parent_row};

/// The label of the synthetic row that re-roots the tree one level up.
pub(crate) const PARENT_ROW_LABEL: &str = "../";

/// The nodes directly under each directory: each child's path, and whether it
/// is a directory.
///
/// Keyed by path, so a node contributed twice collapses into one.
type Children = BTreeMap<PathBuf, BTreeMap<PathBuf, bool>>;

/// The nested rows the tree renders under `root`.
///
/// Off the brain root the list opens with a synthetic `../` leaf whose
/// identifier is the directory it re-roots to, so selecting it needs no
/// special state, only a look at the identifier.
pub(crate) fn build_items(
    entries: &[Entry],
    root: &Path,
    brain_root: &Path,
) -> Vec<TreeItem<'static, PathBuf>> {
    let children = group_children(entries, root);

    let mut items = Vec::new();
    if let Some(parent) = ascend(root, brain_root) {
        debug_assert!(shows_parent_row(root, brain_root));
        items.push(TreeItem::new_leaf(parent, PARENT_ROW_LABEL.to_owned()));
    }
    items.extend(items_under(root, &children));
    items
}

/// Index every entry under its parent, synthesizing the directories in
/// between.
///
/// `entry::collect` skips each walk root, so a bucket directory has no `Entry`
/// of its own even though its contents do. Without synthesizing those the tree
/// would lose whole levels, and at the brain root it would render nothing at
/// all, which is exactly where an unscoped search puts it.
fn group_children(entries: &[Entry], root: &Path) -> Children {
    let mut children: Children = BTreeMap::new();
    for entry in entries
        .iter()
        .filter(|entry| entry.path.starts_with(root) && entry.path != root)
    {
        let Some(parent) = entry.path.parent() else {
            continue;
        };
        children
            .entry(parent.to_path_buf())
            .or_default()
            .insert(entry.path.clone(), entry.is_dir);

        let mut current = parent;
        while current != root {
            let Some(grandparent) = current.parent() else {
                break;
            };
            children
                .entry(grandparent.to_path_buf())
                .or_default()
                .insert(current.to_path_buf(), true);
            if !grandparent.starts_with(root) {
                break;
            }
            current = grandparent;
        }
    }
    children
}

/// The rows directly under `parent`, recursing into each directory.
fn items_under(parent: &Path, children: &Children) -> Vec<TreeItem<'static, PathBuf>> {
    let mut nodes: Vec<(&PathBuf, bool)> = children
        .get(parent)
        .map(|nodes| nodes.iter().map(|(path, dir)| (path, *dir)).collect())
        .unwrap_or_default();
    nodes.sort_by(|(a_path, a_dir), (b_path, b_dir)| {
        b_dir
            .cmp(a_dir)
            .then_with(|| sort_key(a_path).cmp(&sort_key(b_path)))
    });

    nodes
        .into_iter()
        .filter_map(|(path, is_dir)| {
            let label = label_for(path);
            if is_dir {
                // `new` only errors on duplicate child identifiers, which
                // cannot happen: every identifier is a distinct absolute path.
                TreeItem::new(path.clone(), label, items_under(path, children)).ok()
            } else {
                Some(TreeItem::new_leaf(path.clone(), label))
            }
        })
        .collect()
}

/// The row's visible text: just the final path segment.
fn label_for(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Case-insensitive name, so `Yak` and `alpha` sort the way a reader expects.
fn sort_key(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// The widget addresses a node by the full path of identifiers from the root
/// down to it. This builds that path for `target` under `root`.
pub(crate) fn identifier_path(target: &Path, root: &Path) -> Vec<PathBuf> {
    let Ok(relative) = target.strip_prefix(root) else {
        return Vec::new();
    };
    let mut current = root.to_path_buf();
    let mut path = Vec::new();
    for component in relative.components() {
        current = current.join(component);
        path.push(current.clone());
    }
    path
}

/// Every node that must be open for `target` to be visible: each of its
/// ancestors below `root`, addressed by full identifier path.
pub(crate) fn opened_for(target: &Path, root: &Path) -> HashSet<Vec<PathBuf>> {
    let full = identifier_path(target, root);
    // The last element is the target itself, which need not be opened.
    (1..full.len()).map(|len| full[..len].to_vec()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Bucket;

    fn dir(path: &str) -> Entry {
        Entry {
            path: PathBuf::from(path),
            display: path.to_owned(),
            bucket: Bucket::Projects,
            is_dir: true,
        }
    }

    fn file(path: &str) -> Entry {
        Entry {
            path: PathBuf::from(path),
            display: path.to_owned(),
            bucket: Bucket::Projects,
            is_dir: false,
        }
    }

    fn labels(items: &[TreeItem<'static, PathBuf>]) -> Vec<String> {
        items
            .iter()
            .map(|item| item.identifier().display().to_string())
            .collect()
    }

    #[test]
    fn children_nest_under_their_directory() {
        let entries = vec![
            dir("/brain/projects/atlas"),
            file("/brain/projects/atlas/plan.md"),
            file("/brain/projects/loose.md"),
        ];

        let items = build_items(&entries, Path::new("/brain/projects"), Path::new("/brain"));

        assert_eq!(
            labels(&items),
            vec![
                "/brain".to_owned(),
                "/brain/projects/atlas".to_owned(),
                "/brain/projects/loose.md".to_owned(),
            ]
        );
        let atlas = &items[1];
        assert_eq!(atlas.children().len(), 1);
        assert_eq!(
            atlas.children()[0].identifier(),
            &PathBuf::from("/brain/projects/atlas/plan.md")
        );
    }

    #[test]
    fn directories_sort_before_files_then_alphabetically() {
        let entries = vec![
            file("/brain/projects/zebra.md"),
            file("/brain/projects/alpha.md"),
            dir("/brain/projects/Yak"),
            dir("/brain/projects/beta"),
        ];

        let items = build_items(&entries, Path::new("/brain/projects"), Path::new("/brain"));

        assert_eq!(
            labels(&items),
            vec![
                "/brain".to_owned(),
                "/brain/projects/beta".to_owned(),
                "/brain/projects/Yak".to_owned(),
                "/brain/projects/alpha.md".to_owned(),
                "/brain/projects/zebra.md".to_owned(),
            ],
            "the ../ row first, then directories, then files, each case-insensitively"
        );
    }

    #[test]
    fn a_directory_with_nothing_under_it_is_still_a_row() {
        // A row comes from the parent index, not from having children, so a
        // directory the walk found empty is not dropped. It is deliberately
        // *not* asserted to look like a directory: the widget picks its marker
        // from `children.is_empty()`, so an empty directory renders like a
        // leaf. `view::tests` covers the marker a populated one gets.
        let entries = vec![dir("/brain/projects/empty")];

        let items = build_items(&entries, Path::new("/brain/projects"), Path::new("/brain"));

        assert_eq!(
            labels(&items),
            vec!["/brain".to_owned(), "/brain/projects/empty".to_owned()]
        );
        assert!(items[1].children().is_empty());
    }

    #[test]
    fn off_root_the_first_row_re_roots_to_the_parent() {
        let entries = vec![file("/brain/projects/plan.md")];

        let items = build_items(&entries, Path::new("/brain/projects"), Path::new("/brain"));

        assert_eq!(
            items[0].identifier(),
            &PathBuf::from("/brain"),
            "the parent row carries the directory it re-roots to"
        );
        assert!(items[0].children().is_empty());
    }

    #[test]
    fn at_the_brain_root_there_is_no_parent_row() {
        // Entries as `collect` really emits them: nothing for the bucket
        // directory itself, only for what is inside it.
        let entries = vec![file("/brain/projects/plan.md")];

        let items = build_items(&entries, Path::new("/brain"), Path::new("/brain"));

        assert_eq!(
            labels(&items),
            vec!["/brain/projects".to_owned()],
            "nothing above the brain root, so nothing to ascend to"
        );
    }

    #[test]
    fn the_brain_root_lists_the_bucket_directories_the_walk_skipped() {
        // `entry::collect` skips each walk root, so there is no Entry for
        // `/brain/projects` itself, only for what is inside it. The tree has
        // to show it anyway: the default unscoped search spans every bucket,
        // which roots the tree here, and an empty tree would be the whole
        // feature failing on its most common path.
        let entries = vec![
            file("/brain/projects/plan.md"),
            file("/brain/areas/health/log.md"),
        ];

        let items = build_items(&entries, Path::new("/brain"), Path::new("/brain"));

        assert_eq!(
            labels(&items),
            vec!["/brain/areas".to_owned(), "/brain/projects".to_owned()]
        );
        let areas = &items[0];
        assert_eq!(
            areas.children()[0].identifier(),
            &PathBuf::from("/brain/areas/health"),
            "an intermediate directory with no Entry of its own still nests"
        );
        assert_eq!(
            areas.children()[0].children()[0].identifier(),
            &PathBuf::from("/brain/areas/health/log.md")
        );
    }

    #[test]
    fn entries_outside_the_root_are_left_out() {
        let entries = vec![
            file("/brain/projects/plan.md"),
            file("/brain/areas/health/log.md"),
        ];

        let items = build_items(&entries, Path::new("/brain/projects"), Path::new("/brain"));

        assert_eq!(
            labels(&items),
            vec!["/brain".to_owned(), "/brain/projects/plan.md".to_owned()]
        );
    }

    #[test]
    fn opening_a_target_opens_every_ancestor_below_the_root() {
        let opened = opened_for(
            Path::new("/brain/projects/atlas/deep/plan.md"),
            Path::new("/brain/projects"),
        );

        assert_eq!(
            opened,
            HashSet::from([
                vec![PathBuf::from("/brain/projects/atlas")],
                vec![
                    PathBuf::from("/brain/projects/atlas"),
                    PathBuf::from("/brain/projects/atlas/deep"),
                ],
            ]),
            "each opened node is addressed by its full identifier path"
        );
    }

    #[test]
    fn a_target_directly_under_the_root_needs_nothing_opened() {
        assert!(
            opened_for(
                Path::new("/brain/projects/plan.md"),
                Path::new("/brain/projects")
            )
            .is_empty()
        );
    }

    #[test]
    fn the_identifier_path_to_a_target_is_root_relative() {
        assert_eq!(
            identifier_path(
                Path::new("/brain/projects/atlas/plan.md"),
                Path::new("/brain/projects")
            ),
            vec![
                PathBuf::from("/brain/projects/atlas"),
                PathBuf::from("/brain/projects/atlas/plan.md"),
            ]
        );
    }

    #[test]
    fn a_target_outside_the_root_addresses_nothing() {
        assert!(identifier_path(Path::new("/etc/passwd"), Path::new("/brain")).is_empty());
        assert!(opened_for(Path::new("/etc/passwd"), Path::new("/brain")).is_empty());
    }
}
