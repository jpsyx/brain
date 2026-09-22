//! Turning the flat entry list into the nested tree the widget renders.
//!
//! Everything here is pure: it reads `Entry::is_dir` rather than the
//! filesystem, so the whole shape of the tree is testable without a temp dir.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use ratatui::text::{Line, Span};
use tui_tree_widget::TreeItem;

use crate::entry::Entry;
use crate::render;

use super::kind::{FileKind, classify};
use super::root::{ascend, shows_parent_row};

/// The label of the synthetic row that re-roots the tree one level up.
pub(crate) const PARENT_ROW_LABEL: &str = "../";

/// What a row needs beyond its own path: enough to label it, colour it, and
/// sort it.
struct Node {
    is_dir: bool,
    hidden: bool,
}

/// The nodes directly under each directory, keyed by path so a node
/// contributed twice collapses into one.
type Children = BTreeMap<PathBuf, BTreeMap<PathBuf, Node>>;

/// The nested rows the tree renders under `root`.
///
/// Off the brain root the list opens with a synthetic `../` leaf whose
/// identifier is the directory it re-roots to, so selecting it needs no
/// special state, only a look at the identifier. `show_hidden` decides whether
/// dotted names are rows at all; the `../` row is navigation and is never one
/// of them.
pub(crate) fn build_items(
    entries: &[Entry],
    root: &Path,
    brain_root: &Path,
    show_hidden: bool,
) -> Vec<TreeItem<'static, PathBuf>> {
    let children = group_children(entries, root, show_hidden);

    let mut items = Vec::new();
    if let Some(parent) = ascend(root, brain_root) {
        debug_assert!(shows_parent_row(root, brain_root));
        items.push(TreeItem::new_leaf(parent, parent_row_line()));
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
///
/// This is also where a hidden entry stops being a row: dropping it here drops
/// the ancestors it would otherwise have synthesized, so a dotted directory
/// nobody visible sits under disappears with its contents.
fn group_children(entries: &[Entry], root: &Path, show_hidden: bool) -> Children {
    let mut children: Children = BTreeMap::new();
    for entry in entries.iter().filter(|entry| {
        entry.path.starts_with(root) && entry.path != root && (show_hidden || !entry.is_hidden)
    }) {
        let Some(parent) = entry.path.parent() else {
            continue;
        };
        children.entry(parent.to_path_buf()).or_default().insert(
            entry.path.clone(),
            Node {
                is_dir: entry.is_dir,
                hidden: entry.is_hidden,
            },
        );

        let mut current = parent;
        while current != root {
            let Some(grandparent) = current.parent() else {
                break;
            };
            children
                .entry(grandparent.to_path_buf())
                .or_default()
                .insert(
                    current.to_path_buf(),
                    Node {
                        is_dir: true,
                        hidden: crate::entry::hidden_below(root, current),
                    },
                );
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
    let mut nodes: Vec<(&PathBuf, &Node)> = children
        .get(parent)
        .map(|nodes| nodes.iter().collect())
        .unwrap_or_default();
    nodes.sort_by(|(a_path, a_node), (b_path, b_node)| {
        b_node
            .is_dir
            .cmp(&a_node.is_dir)
            .then_with(|| sort_key(a_path).cmp(&sort_key(b_path)))
    });

    nodes
        .into_iter()
        .filter_map(|(path, node)| {
            let line = row_line(path, classify(path, node.is_dir), node.hidden);
            if node.is_dir {
                // `new` only errors on duplicate child identifiers, which
                // cannot happen: every identifier is a distinct absolute path.
                TreeItem::new(path.clone(), line, items_under(path, children)).ok()
            } else {
                Some(TreeItem::new_leaf(path.clone(), line))
            }
        })
        .collect()
}

/// One row, coloured by what it is and dimmed when it is normally out of sight.
fn row_line(path: &Path, kind: FileKind, hidden: bool) -> Line<'static> {
    Line::from(Span::styled(
        label_for(path, kind == FileKind::Directory),
        render::tree_row_style(kind, hidden),
    ))
}

/// The synthetic `../` row, styled as navigation rather than as content.
fn parent_row_line() -> Line<'static> {
    Line::from(Span::styled(
        PARENT_ROW_LABEL,
        render::tree_row_style(FileKind::ParentRow, false),
    ))
}

/// The row's visible text: the final path segment.
///
/// A directory gains a trailing `/` so a level still reads as folders-then-
/// files with colour off, in a pipe, or on a terminal that drops it.
fn label_for(path: &Path, is_dir: bool) -> String {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    if is_dir { format!("{name}/") } else { name }
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
    use ratatui::style::{Modifier, Style};

    fn dir(path: &str) -> Entry {
        entry(path, true)
    }

    fn file(path: &str) -> Entry {
        entry(path, false)
    }

    /// `is_hidden` is derived from the path, exactly as `entry::collect`
    /// derives it, so a dotted fixture needs no extra argument to be honest.
    fn entry(path: &str, is_dir: bool) -> Entry {
        let path = PathBuf::from(path);
        let display = path.display().to_string();
        let is_hidden = crate::entry::hidden_below(Path::new("/brain"), &path);
        Entry {
            path,
            display,
            bucket: Bucket::Projects,
            is_dir,
            is_hidden,
        }
    }

    /// The text of a single-span row line.
    fn text_of(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// The style of a single-span row line.
    fn style_of(line: &Line<'static>) -> Style {
        line.spans.first().expect("a row has one span").style
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

        let items = build_items(
            &entries,
            Path::new("/brain/projects"),
            Path::new("/brain"),
            false,
        );

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

        let items = build_items(
            &entries,
            Path::new("/brain/projects"),
            Path::new("/brain"),
            false,
        );

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

        let items = build_items(
            &entries,
            Path::new("/brain/projects"),
            Path::new("/brain"),
            false,
        );

        assert_eq!(
            labels(&items),
            vec!["/brain".to_owned(), "/brain/projects/empty".to_owned()]
        );
        assert!(items[1].children().is_empty());
    }

    #[test]
    fn off_root_the_first_row_re_roots_to_the_parent() {
        let entries = vec![file("/brain/projects/plan.md")];

        let items = build_items(
            &entries,
            Path::new("/brain/projects"),
            Path::new("/brain"),
            false,
        );

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

        let items = build_items(&entries, Path::new("/brain"), Path::new("/brain"), false);

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

        let items = build_items(&entries, Path::new("/brain"), Path::new("/brain"), false);

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

        let items = build_items(
            &entries,
            Path::new("/brain/projects"),
            Path::new("/brain"),
            false,
        );

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

    #[test]
    fn a_directory_label_ends_with_a_slash_and_a_file_label_does_not() {
        assert_eq!(
            label_for(Path::new("/brain/projects/atlas"), true),
            "atlas/"
        );
        assert_eq!(
            label_for(Path::new("/brain/projects/plan.md"), false),
            "plan.md"
        );
    }

    #[test]
    fn a_directory_row_is_cyan_bold_and_slashed() {
        let line = row_line(
            Path::new("/brain/projects/atlas"),
            FileKind::Directory,
            false,
        );

        assert_eq!(text_of(&line), "atlas/");
        let style = style_of(&line);
        assert_eq!(style.fg, Some(render::ACCENT_CYAN));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_hidden_row_is_dimmed_but_keeps_its_kind_colour() {
        let path = Path::new("/brain/projects/.config.json");
        let line = row_line(path, classify(path, false), true);

        let style = style_of(&line);
        assert_eq!(
            style.fg,
            Some(render::ACCENT_YELLOW),
            "a hidden data file must still read as data"
        );
        assert!(style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn the_parent_row_is_styled_as_navigation_not_content() {
        let line = parent_row_line();

        assert_eq!(text_of(&line), PARENT_ROW_LABEL);
        assert_eq!(style_of(&line).fg, Some(render::TEXT_VERY_DIM));
    }

    #[test]
    fn a_synthesized_ancestor_and_everything_under_it_read_as_hidden() {
        // `/brain/projects/.config` has no Entry of its own: it is in the tree
        // only because the note inside it is. Both must read as hidden, and by
        // the *same* rule — a component below the root that starts with a dot.
        // Read off the final segment alone, `app.json` would be an ordinary
        // row and would drag its dotted directory back on screen as its
        // parent.
        let entries = vec![file("/brain/projects/.config/app.json")];

        let children = group_children(&entries, Path::new("/brain"), true);

        let config = &children[Path::new("/brain/projects")][Path::new("/brain/projects/.config")];
        assert!(config.is_dir);
        assert!(config.hidden, "a dotted ancestor is hidden");

        let projects = &children[Path::new("/brain")][Path::new("/brain/projects")];
        assert!(
            !projects.hidden,
            "an ordinary ancestor above a dotted one is not hidden"
        );

        let note = &children[Path::new("/brain/projects/.config")]
            [Path::new("/brain/projects/.config/app.json")];
        assert!(
            note.hidden,
            "a note inside a dotted directory is not visible content"
        );
    }

    #[test]
    fn hidden_entries_are_not_rows_until_they_are_asked_for() {
        // The toggle's whole effect, at the level where it is decided.
        let entries = vec![
            file("/brain/projects/plan.md"),
            file("/brain/projects/.secret.md"),
            file("/brain/projects/.obsidian/notes.md"),
        ];
        let root = Path::new("/brain/projects");

        let visible = group_children(&entries, root, false);
        assert_eq!(
            visible[root].keys().map(PathBuf::as_path).collect::<Vec<_>>(),
            vec![Path::new("/brain/projects/plan.md")],
            "the dotfile and the whole dotted directory drop out together"
        );

        let all = group_children(&entries, root, true);
        assert_eq!(
            all[root].keys().map(PathBuf::as_path).collect::<Vec<_>>(),
            vec![
                Path::new("/brain/projects/.obsidian"),
                Path::new("/brain/projects/.secret.md"),
                Path::new("/brain/projects/plan.md"),
            ]
        );
    }

    #[test]
    fn build_items_hides_dotted_rows_unless_they_are_shown() {
        let entries = vec![
            file("/brain/projects/plan.md"),
            file("/brain/projects/.obsidian/notes.md"),
        ];

        let hidden = build_items(
            &entries,
            Path::new("/brain/projects"),
            Path::new("/brain"),
            false,
        );
        assert_eq!(
            labels(&hidden),
            vec!["/brain".to_owned(), "/brain/projects/plan.md".to_owned()],
            "the ../ row is navigation and stays either way"
        );

        let shown = build_items(
            &entries,
            Path::new("/brain/projects"),
            Path::new("/brain"),
            true,
        );
        assert_eq!(
            labels(&shown),
            vec![
                "/brain".to_owned(),
                "/brain/projects/.obsidian".to_owned(),
                "/brain/projects/plan.md".to_owned(),
            ]
        );
        assert_eq!(
            shown[1].children()[0].identifier(),
            &PathBuf::from("/brain/projects/.obsidian/notes.md"),
            "what is inside a shown dotted directory is shown with it"
        );
    }
}
