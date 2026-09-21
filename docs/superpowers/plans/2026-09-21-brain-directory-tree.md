# Brain-Directory Tree Sub-View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a directory-tree sub-view to brain's brain-directory main view, reached with `Alt+Enter` from the fuzzy-search sub-view and from a new `Explore` command-palette row.

**Architecture:** A new pure `src/tree/` module (mirroring `src/picker/`) owns the tree model, root resolution, and key decisions. `ShellState` gains a `BrainDirView { Search, Tree }` axis; the existing `SearchEffect` enum grows three variants and becomes the brain-directory effect enum for both sub-views. One new `EntryCommand::Explore` variant produces the palette row in every view automatically, because the palette is already the parent set of every command.

**Tech Stack:** Rust 2024, ratatui 0.29, crossterm 0.28, `tui-tree-widget` 0.23.1 (pinned: 0.24 requires the ratatui 0.30 split).

**Spec:** `docs/superpowers/specs/2026-09-21-brain-directory-tree-design.md`

---

## Ground rules for every task

This repository has an **iron law**: no production code lands without a failing
test written first. Every task below is RED (write the test, run it, watch it
fail) → GREEN (minimal code) → commit. Do not skip the "run it and watch it
fail" step; a test you never saw fail proves nothing.

Commands you will use constantly:

```sh
cd /Users/juanpablosarmiento/src/worktrees/jpsyx/feat/tree-view

cargo test --release                       # the whole suite, well under a second
cargo test --release tree::                # one module
cargo clippy --release --all-targets -- -D warnings
```

`pedantic` + `nursery` clippy lints are on at warn and the build treats
warnings as errors in CI. `unsafe` is forbidden crate-wide. Keep every file
under roughly 400 lines of production code.

**Do not bump the crate version in individual task commits.** Task 11 does it
once, at the end.

---

## File Structure

| File | Status | Responsibility |
| --- | --- | --- |
| `Cargo.toml` | Modify | Add `tui-tree-widget = "0.23.1"` with a justifying comment; version bump in Task 11 |
| `src/entry.rs` | Modify | Add `Entry::is_dir`, populated by `walkdir` during the walk |
| `src/picker/selection.rs` | Modify | Read `entry.is_dir` instead of calling `path.is_file()` / `path.is_dir()` |
| `src/tree/mod.rs` | Create | `TreeView` state: root, `TreeState<PathBuf>`, built items |
| `src/tree/root.rs` | Create | `scope_root`, `ascend`, `shows_parent_row` |
| `src/tree/build.rs` | Create | Entries + root → `Vec<TreeItem<'static, PathBuf>>` and the opened-set |
| `src/tree/input.rs` | Create | `handle_tree_input` → `SearchEffect` |
| `src/tree/view.rs` | Create | `draw_into` |
| `src/lib.rs` | Modify | `mod tree;` |
| `src/tui/state/shell.rs` | Modify | `BrainDirView` axis, `SearchEffect` variants, tree accessors, `Alt+Enter` in search input |
| `src/tui/palette/command/mod.rs` | Modify | `EntryCommand::Explore` + its requirement, shortcut, picker title |
| `src/tui/palette/command/catalog.rs` | Modify | List `Explore` in the brain-directory group |
| `src/tui/palette/command/naming.rs` | Modify | Named and generic labels for `Explore` |
| `src/tui/palette/command/labels.rs` | Modify | `explore_label` |
| `src/tui/app_actions/entry_commands.rs` | Modify | Run `Explore` |
| `src/tui/tree_view.rs` | Create | Glue: apply tree effects, mirrors `search_view.rs` |
| `src/tui/draw/mod.rs` | Modify | Dispatch the brain-directory area to search or tree |
| `src/tui/event_loop/run.rs` | Modify | Route keys to the active sub-view |
| `src/tasks/shortcuts/table.rs` | Modify | New brain-directory rows |
| `src/tasks/shortcuts/tests.rs` | Modify | Update the two pinned guard lists |
| `docs/*.md` | Modify | The docs contract, Task 11 |

---

### Task 1: `Entry::is_dir`

The tree must know whether a path is a directory without touching the
filesystem, so the whole build stays pure and testable. `walkdir` already knows
this during the walk, so it costs nothing to record. It also removes a
per-selection `is_file()` syscall from the picker.

**Files:**
- Modify: `src/entry.rs`
- Modify: `src/picker/selection.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/entry.rs` (if
there is no such block, create one at the end of the file):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_records_whether_each_entry_is_a_directory() {
        let temp = tempfile::tempdir().expect("temp dir");
        let brain = temp.path();
        let projects = brain.join("projects");
        std::fs::create_dir_all(projects.join("atlas")).expect("create nested dir");
        std::fs::write(projects.join("atlas/plan.md"), "# plan").expect("write file");

        let entries = collect(brain, &[(Bucket::Projects, projects)]).expect("collect");

        let dir = entries
            .iter()
            .find(|e| e.path.ends_with("atlas"))
            .expect("the atlas directory is collected");
        let file = entries
            .iter()
            .find(|e| e.path.ends_with("plan.md"))
            .expect("the plan file is collected");

        assert!(dir.is_dir, "a directory entry must be marked as one");
        assert!(!file.is_dir, "a file entry must not be marked as a directory");
    }
}
```

- [ ] **Step 2: Run the test and verify it fails**

```sh
cargo test --release entry::tests::collect_records_whether_each_entry_is_a_directory
```

Expected: a compile error, `no field 'is_dir' on type 'Entry'`.

If `tempfile` is not already usable from `src/`, note that it is a
`[dev-dependencies]` entry in `Cargo.toml`, which makes it available to
`#[cfg(test)]` code. No Cargo change is needed.

- [ ] **Step 3: Add the field and populate it**

In `src/entry.rs`, add the field to the struct:

```rust
#[derive(Debug, Clone)]
pub struct Entry {
    /// Absolute path on disk. Passed to `open` when the user selects.
    pub path: PathBuf,
    /// `~/brain/...` form for display + fuzzy matching.
    pub display: String,
    /// Which PARA bucket this entry belongs to.
    pub bucket: Bucket,
    /// Whether this entry is a directory. Recorded during the walk, where
    /// `walkdir` already knows, so consumers (the tree, the picker's palette
    /// context) never need a syscall to ask.
    pub is_dir: bool,
}
```

In `collect`, capture the file type before `entry.into_path()` consumes it:

```rust
            let display = display_path(brain, entry.path());
            let is_dir = entry.file_type().is_dir();
            out.push(Entry {
                path: entry.into_path(),
                display,
                bucket: *bucket,
                is_dir,
            });
```

- [ ] **Step 4: Run the test and verify it passes**

```sh
cargo test --release entry::tests::collect_records_whether_each_entry_is_a_directory
```

Expected: PASS.

- [ ] **Step 5: Fix every other construction site**

```sh
cargo test --release 2>&1 | head -60
```

Every place that builds an `Entry` literal now fails to compile. Add
`is_dir: false` to test fixtures that build plain files, and `is_dir: true`
where the fixture means a directory. Then switch `src/picker/selection.rs` to
read the recorded flag rather than the filesystem:

In `selected_entry_context`, replace `let is_file = path.is_file();` with a
lookup through the selected match:

```rust
    pub(crate) fn selected_entry_context(&self) -> Option<EntryContext> {
        let path = self.selected_path()?;
        let is_file = !self.selected_is_dir()?;
        Some(EntryContext {
            filename: self.selected_filename()?,
            dir_reldisplay: self.selected_dir_reldisplay().unwrap_or_default(),
            is_file,
            is_markdown: is_file && open_target::is_markdown(&path),
        })
    }

    /// Whether the highlighted entry is a directory, from the flag recorded
    /// during the walk rather than a fresh syscall.
    pub(crate) fn selected_is_dir(&self) -> Option<bool> {
        let m = self.matches.get(self.selected)?;
        Some(self.entries[m.entry_idx].is_dir)
    }
```

And in `selected_dir_reldisplay`, replace `entry.path.is_dir()` with
`entry.is_dir`:

```rust
        Some(if entry.is_dir {
            rel
        } else {
            parent_reldisplay(&rel)
        })
```

- [ ] **Step 6: Run the full suite and clippy**

```sh
cargo test --release && cargo clippy --release --all-targets -- -D warnings
```

Expected: all green.

- [ ] **Step 7: Commit**

```sh
git add -A
git commit -m "feat: record is_dir on Entry during the walk

The tree sub-view needs to know a path's kind without a syscall, so the
whole build can stay pure. walkdir already knows during the walk, and the
picker's palette context drops a per-selection is_file() call.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Add the `tui-tree-widget` dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add the dependency**

In `Cargo.toml`, in `[dependencies]`, in alphabetical position (after
`serde_json`/`sha1`/`sha2`/`signal-hook`, before `tui-term`):

```toml
# The brain-directory tree sub-view. Pinned to 0.23.x: 0.24 moved to
# ratatui-core 0.1 + ratatui-widgets 0.3, the ratatui 0.30 split, which this
# crate has not taken yet. The widget is state-only (TreeState exposes
# key_up/key_down/key_left/key_right, open/close/toggle, selected) with no
# event handling and no filesystem access, which is what lets brain keep the
# walk, the keymap, and every decision in its own pure functions.
# See docs/decisions.md and docs/architecture.md.
tui-tree-widget = "0.23.1"
```

- [ ] **Step 2: Verify it resolves against ratatui 0.29**

```sh
cargo tree --package brain --invert ratatui 2>&1 | head -20
cargo build --release 2>&1 | tail -5
```

Expected: exactly one `ratatui v0.29.x` in the tree, and a clean build. If
cargo pulls a second ratatui, stop: the pin is wrong and the rest of the plan
does not apply.

- [ ] **Step 3: Commit**

```sh
git add Cargo.toml Cargo.lock
git commit -m "build: add tui-tree-widget 0.23.1 for the directory tree

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `src/tree/root.rs` — root resolution

Three pure functions. `scope_root` derives the tree's starting root from the
entries the picker holds (there is no stored scope). `ascend` walks up one
level, refusing to leave the brain root. `shows_parent_row` is the render-side
question that must agree with `ascend`.

**Files:**
- Create: `src/tree/root.rs`
- Create: `src/tree/mod.rs` (stub for now; Task 5 fills it in)
- Modify: `src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `src/tree/root.rs` containing only the test module for now:

```rust
//! Where the tree is rooted, and how far up it may walk.
//!
//! The brain root is the ceiling: `ascend` refuses to return a parent at or
//! above it, which is what makes the `../` row disappear there and what keeps
//! the tree inside the workspace no matter how it is entered.

use std::path::{Path, PathBuf};

use crate::entry::Entry;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Bucket;

    fn entry(path: &str, bucket: Bucket) -> Entry {
        Entry {
            path: PathBuf::from(path),
            display: path.to_owned(),
            bucket,
            is_dir: false,
        }
    }

    #[test]
    fn one_bucket_in_scope_roots_at_that_bucket_directory() {
        let entries = vec![
            entry("/brain/projects/atlas/plan.md", Bucket::Projects),
            entry("/brain/projects/atlas", Bucket::Projects),
        ];
        assert_eq!(
            scope_root(&entries, Path::new("/brain")),
            PathBuf::from("/brain/projects")
        );
    }

    #[test]
    fn several_buckets_in_scope_root_at_the_brain_root() {
        let entries = vec![
            entry("/brain/projects/atlas", Bucket::Projects),
            entry("/brain/areas/health", Bucket::Areas),
        ];
        assert_eq!(
            scope_root(&entries, Path::new("/brain")),
            PathBuf::from("/brain")
        );
    }

    #[test]
    fn no_entries_root_at_the_brain_root() {
        assert_eq!(scope_root(&[], Path::new("/brain")), PathBuf::from("/brain"));
    }

    #[test]
    fn ascend_returns_the_parent_below_the_brain_root() {
        assert_eq!(
            ascend(Path::new("/brain/projects/atlas"), Path::new("/brain")),
            Some(PathBuf::from("/brain/projects"))
        );
        assert_eq!(
            ascend(Path::new("/brain/projects"), Path::new("/brain")),
            Some(PathBuf::from("/brain"))
        );
    }

    #[test]
    fn ascend_refuses_to_leave_the_brain_root() {
        // At the ceiling there is nowhere to go.
        assert_eq!(ascend(Path::new("/brain"), Path::new("/brain")), None);
        // And a path that was never inside it cannot climb into one.
        assert_eq!(ascend(Path::new("/etc/passwd"), Path::new("/brain")), None);
        assert_eq!(ascend(Path::new("/"), Path::new("/brain")), None);
    }

    #[test]
    fn the_parent_row_shows_exactly_when_there_is_somewhere_to_ascend_to() {
        for (root, expected) in [
            ("/brain/projects/atlas", true),
            ("/brain/projects", true),
            ("/brain", false),
            ("/etc", false),
        ] {
            let root = Path::new(root);
            let brain = Path::new("/brain");
            assert_eq!(shows_parent_row(root, brain), expected, "{root:?}");
            assert_eq!(shows_parent_row(root, brain), ascend(root, brain).is_some());
        }
    }
}
```

Create `src/tree/mod.rs`:

```rust
//! The brain-directory tree sub-view's pure model.

pub(crate) mod root;
```

Add to `src/lib.rs`, in alphabetical position among the other `mod`
declarations:

```rust
mod tree;
```

- [ ] **Step 2: Run the tests and verify they fail**

```sh
cargo test --release tree::root
```

Expected: compile errors, `cannot find function 'scope_root'`, `cannot find
function 'ascend'`, `cannot find function 'shows_parent_row'`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` block in `src/tree/root.rs`:

```rust
/// Where the tree opens for the search scope currently loaded.
///
/// `picker::App` stores no scope, only entries, each tagged with its bucket.
/// Entries spanning exactly one bucket mean a scoped search, so the tree opens
/// at that bucket's directory; anything else (several buckets, or none) opens
/// at the brain root. Deriving it keeps one source of truth, rather than a
/// stored scope that could drift from the entries themselves.
pub(crate) fn scope_root(entries: &[Entry], brain_root: &Path) -> PathBuf {
    let mut buckets = entries.iter().map(|entry| entry.bucket);
    let Some(first) = buckets.next() else {
        return brain_root.to_path_buf();
    };
    if buckets.any(|bucket| bucket != first) {
        return brain_root.to_path_buf();
    }
    brain_root.join(first.label().to_ascii_lowercase())
}

/// The directory one level above `root`, or `None` when there is nowhere
/// legal to go: `root` is the brain root, or sits outside it entirely.
///
/// This is the single place the "never above the brain root" rule lives.
pub(crate) fn ascend(root: &Path, brain_root: &Path) -> Option<PathBuf> {
    if !root.starts_with(brain_root) || root == brain_root {
        return None;
    }
    root.parent()
        .filter(|parent| parent.starts_with(brain_root))
        .map(Path::to_path_buf)
}

/// Whether the tree shows its synthetic `../` row. Defined in terms of
/// [`ascend`] so the row can never offer a move the model would refuse.
pub(crate) fn shows_parent_row(root: &Path, brain_root: &Path) -> bool {
    ascend(root, brain_root).is_some()
}
```

Note `Bucket::label()` returns `"Capture"` / `"Projects"` / `"Areas"` /
`"Resources"` / `"Archive"`, and the on-disk directories are those names
lowercased, which is exactly what `search_view::single_bucket_root` does.

- [ ] **Step 4: Run the tests and verify they pass**

```sh
cargo test --release tree::root
```

Expected: 6 passing tests.

- [ ] **Step 5: Commit**

```sh
git add src/tree src/lib.rs
git commit -m "feat: resolve the tree's root and its brain-root ceiling

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: `src/tree/build.rs` — entries to tree items

Builds the nested `TreeItem` structure from the flat entry list, plus the set
of nodes to pre-open so the tree lands already expanded on the entry the user
was pointing at.

**Files:**
- Create: `src/tree/build.rs`
- Modify: `src/tree/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `src/tree/build.rs` with the test module:

```rust
//! Turning the flat entry list into the nested tree the widget renders.
//!
//! Everything here is pure: it reads `Entry::is_dir` rather than the
//! filesystem, so the whole shape of the tree is testable without a temp dir.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use tui_tree_widget::TreeItem;

use crate::entry::Entry;

use super::root::shows_parent_row;

/// The label of the synthetic row that re-roots the tree one level up.
pub(crate) const PARENT_ROW_LABEL: &str = "../";

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
                "/brain/projects/atlas".to_owned(),
                "/brain/projects/loose.md".to_owned(),
            ]
        );
        let atlas = &items[0];
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
                "/brain/projects/beta".to_owned(),
                "/brain/projects/Yak".to_owned(),
                "/brain/projects/alpha.md".to_owned(),
                "/brain/projects/zebra.md".to_owned(),
            ],
            "directories first, then case-insensitive name"
        );
    }

    #[test]
    fn an_empty_directory_is_still_a_node_with_no_children() {
        // is_dir comes from the walk, so an empty directory is not mistaken
        // for a file just because nothing nests under it.
        let entries = vec![dir("/brain/projects/empty")];

        let items = build_items(&entries, Path::new("/brain/projects"), Path::new("/brain"));

        assert_eq!(items.len(), 1);
        assert!(items[0].children().is_empty());
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
        let entries = vec![dir("/brain/projects")];

        let items = build_items(&entries, Path::new("/brain"), Path::new("/brain"));

        assert_eq!(
            labels(&items),
            vec!["/brain/projects".to_owned()],
            "nothing above the brain root, so nothing to ascend to"
        );
    }

    #[test]
    fn entries_outside_the_root_are_left_out() {
        let entries = vec![
            file("/brain/projects/plan.md"),
            file("/brain/areas/health/log.md"),
        ];

        let items = build_items(&entries, Path::new("/brain/projects"), Path::new("/brain"));

        assert_eq!(labels(&items), vec!["/brain/projects/plan.md".to_owned()]);
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
}
```

Add to `src/tree/mod.rs`:

```rust
pub(crate) mod build;
pub(crate) mod root;
```

- [ ] **Step 2: Run the tests and verify they fail**

```sh
cargo test --release tree::build
```

Expected: compile errors for `build_items`, `opened_for`, `identifier_path`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` block in `src/tree/build.rs`:

```rust
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
    let mut children: BTreeMap<PathBuf, Vec<&Entry>> = BTreeMap::new();
    for entry in entries
        .iter()
        .filter(|entry| entry.path.starts_with(root) && entry.path != root)
    {
        if let Some(parent) = entry.path.parent() {
            children.entry(parent.to_path_buf()).or_default().push(entry);
        }
    }

    let mut items = Vec::new();
    if let Some(parent) = crate::tree::root::ascend(root, brain_root) {
        debug_assert!(shows_parent_row(root, brain_root));
        items.push(TreeItem::new_leaf(parent, PARENT_ROW_LABEL.to_owned()));
    }
    items.extend(items_under(root, &children));
    items
}

/// The rows directly under `parent`, recursing into each directory.
fn items_under(
    parent: &Path,
    children: &BTreeMap<PathBuf, Vec<&Entry>>,
) -> Vec<TreeItem<'static, PathBuf>> {
    let mut entries = children.get(parent).cloned().unwrap_or_default();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| sort_key(&a.path).cmp(&sort_key(&b.path)))
    });

    entries
        .into_iter()
        .filter_map(|entry| {
            let label = label_for(&entry.path);
            if entry.is_dir {
                // `new` only errors on duplicate child identifiers, which
                // cannot happen: every identifier is a distinct absolute path.
                TreeItem::new(
                    entry.path.clone(),
                    label,
                    items_under(&entry.path, children),
                )
                .ok()
            } else {
                Some(TreeItem::new_leaf(entry.path.clone(), label))
            }
        })
        .collect()
}

/// A directory renders with a trailing separator so it reads as one even
/// before it is expanded.
fn label_for(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |name| {
            name.to_string_lossy().into_owned()
        })
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
```

- [ ] **Step 4: Run the tests and verify they pass**

```sh
cargo test --release tree::build
```

Expected: 9 passing tests. If `directories_sort_before_files_then_alphabetically`
fails, check the comparator: `b.is_dir.cmp(&a.is_dir)` puts `true` first
because `false < true`.

- [ ] **Step 5: Run clippy and commit**

```sh
cargo clippy --release --all-targets -- -D warnings
git add src/tree
git commit -m "feat: build the directory tree from the collected entries

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: `src/tree/mod.rs` — the `TreeView` state

**Files:**
- Modify: `src/tree/mod.rs`

- [ ] **Step 1: Write the failing tests**

Replace `src/tree/mod.rs` with the module doc, declarations, the struct, and
this test block:

```rust
//! The brain-directory tree sub-view's pure model.
//!
//! [`TreeView`] pairs the widget's own `TreeState` with the root the tree is
//! currently showing and the items built for it. Every decision about *what*
//! the tree contains lives in `build` and `root`; this type only holds the
//! result and hands the selection back as a path.

pub(crate) mod build;
pub(crate) mod input;
pub(crate) mod root;
pub(crate) mod view;

use std::path::{Path, PathBuf};

use tui_tree_widget::{TreeItem, TreeState};

use crate::entry::Entry;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Bucket;

    fn entry(path: &str, is_dir: bool) -> Entry {
        Entry {
            path: PathBuf::from(path),
            display: path.to_owned(),
            bucket: Bucket::Projects,
            is_dir,
        }
    }

    fn entries() -> Vec<Entry> {
        vec![
            entry("/brain/projects/atlas", true),
            entry("/brain/projects/atlas/plan.md", false),
            entry("/brain/projects/loose.md", false),
        ]
    }

    #[test]
    fn exploring_an_entry_roots_at_the_scope_and_selects_that_entry() {
        let view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/atlas/plan.md"),
        );

        assert_eq!(view.root(), Path::new("/brain/projects"));
        assert_eq!(
            view.selected_path(),
            Some(PathBuf::from("/brain/projects/atlas/plan.md"))
        );
    }

    #[test]
    fn exploring_opens_the_ancestors_so_the_target_is_visible() {
        let view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/atlas/plan.md"),
        );

        assert!(
            view.state
                .opened()
                .contains(&vec![PathBuf::from("/brain/projects/atlas")]),
            "the target's directory must be open for it to be selectable"
        );
    }

    #[test]
    fn re_rooting_moves_the_root_and_rebuilds() {
        let mut view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/loose.md"),
        );
        assert_eq!(view.root(), Path::new("/brain/projects"));

        view.reroot(&entries(), Path::new("/brain"), Path::new("/brain"));

        assert_eq!(view.root(), Path::new("/brain"));
    }

    #[test]
    fn the_parent_row_is_reported_as_a_re_root_not_an_entry() {
        let view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/loose.md"),
        );

        assert!(view.is_parent_row(Path::new("/brain")));
        assert!(!view.is_parent_row(Path::new("/brain/projects/loose.md")));
    }

    #[test]
    fn a_tree_at_the_brain_root_has_no_parent_row_to_confuse_an_entry_with() {
        let mut view = TreeView::explore(
            &entries(),
            Path::new("/brain"),
            Path::new("/brain/projects/loose.md"),
        );
        view.reroot(&entries(), Path::new("/brain"), Path::new("/brain"));

        assert!(!view.is_parent_row(Path::new("/brain")));
    }
}
```

- [ ] **Step 2: Run the tests and verify they fail**

```sh
cargo test --release tree::tests
```

Expected: compile errors for `TreeView`, and for the not-yet-created
`input` and `view` modules. Create empty placeholder files so the module
declarations resolve:

```sh
printf '//! Placeholder, filled in by the next task.\n' > src/tree/input.rs
printf '//! Placeholder, filled in by a later task.\n' > src/tree/view.rs
```

Re-run; the remaining failures must be about `TreeView` itself.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` block in `src/tree/mod.rs`:

```rust
/// The tree sub-view's state: where it is rooted, what it is showing, and
/// where the cursor sits.
pub(crate) struct TreeView {
    root: PathBuf,
    items: Vec<TreeItem<'static, PathBuf>>,
    state: TreeState<PathBuf>,
    /// The directory the synthetic `../` row re-roots to, when there is one.
    /// Held so a selection can be told apart from a real entry by identity
    /// rather than by label.
    parent_row: Option<PathBuf>,
}

impl TreeView {
    /// An empty tree rooted at the brain root, for a shell that has not opened
    /// the sub-view yet.
    pub(crate) fn empty(brain_root: &Path) -> Self {
        Self {
            root: brain_root.to_path_buf(),
            items: Vec::new(),
            state: TreeState::default(),
            parent_row: None,
        }
    }

    /// Open the tree on `target`: rooted at the scope the entries describe,
    /// expanded along the target's ancestors, with the target selected.
    pub(crate) fn explore(entries: &[Entry], brain_root: &Path, target: &Path) -> Self {
        let root = root::scope_root(entries, brain_root);
        let mut view = Self::empty(brain_root);
        view.rebuild(entries, brain_root, &root);
        for opened in build::opened_for(target, &view.root) {
            view.state.open(opened);
        }
        let identifier = build::identifier_path(target, &view.root);
        if !identifier.is_empty() {
            view.state.select(identifier);
        }
        view
    }

    /// Move the root to `root` and rebuild from `entries`, keeping nothing:
    /// a re-root is a different tree, and carrying an old selection into it
    /// would point at a node that may no longer be there.
    pub(crate) fn reroot(&mut self, entries: &[Entry], brain_root: &Path, root: &Path) {
        self.state = TreeState::default();
        self.rebuild(entries, brain_root, root);
    }

    /// Rebuild the items for `root` without touching the selection, for a
    /// refresh in place.
    pub(crate) fn rebuild(&mut self, entries: &[Entry], brain_root: &Path, root: &Path) {
        self.root = root.to_path_buf();
        self.items = build::build_items(entries, root, brain_root);
        self.parent_row = root::ascend(root, brain_root);
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn items(&self) -> &[TreeItem<'static, PathBuf>] {
        &self.items
    }

    pub(crate) const fn state_mut(&mut self) -> &mut TreeState<PathBuf> {
        &mut self.state
    }

    /// The selected node's path, whether it is an entry or the `../` row.
    pub(crate) fn selected_path(&self) -> Option<PathBuf> {
        self.state.selected().last().cloned()
    }

    /// Whether `path` is the synthetic `../` row rather than a real entry.
    /// Selecting it re-roots; selecting anything else acts on a file or
    /// directory.
    pub(crate) fn is_parent_row(&self, path: &Path) -> bool {
        self.parent_row.as_deref() == Some(path)
    }
}
```

- [ ] **Step 4: Run the tests and verify they pass**

```sh
cargo test --release tree::tests
```

Expected: 5 passing tests.

- [ ] **Step 5: Commit**

```sh
cargo clippy --release --all-targets -- -D warnings
git add src/tree
git commit -m "feat: add the TreeView state for the directory sub-view

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: `EntryCommand::Explore`

One new variant gives the palette row in every view, with contextual wording
and a target picker when nothing is highlighted, because the catalog already
lists every declared command unconditionally.

**Files:**
- Modify: `src/tui/palette/command/mod.rs`
- Modify: `src/tui/palette/command/catalog.rs`
- Modify: `src/tui/palette/command/naming.rs`
- Modify: `src/tui/palette/command/labels.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in
`src/tui/palette/command/labels.rs`:

```rust
    #[test]
    fn explore_label_names_the_entry_and_elides_a_long_one() {
        assert_eq!(explore_label("atlas"), "Explore 'atlas'");

        let shown = explore_label("really-long-note-name-that-overflows.md")
            .trim_start_matches("Explore '")
            .trim_end_matches('\'')
            .to_owned();
        assert_eq!(shown.chars().count(), LABEL_MAX_FILENAME);
    }
```

Add to the `#[cfg(test)] mod tests` block in
`src/tui/palette/command/catalog.rs`:

```rust
    #[test]
    fn exploring_an_entry_is_a_listed_command() {
        // The tree sub-view has to be reachable without the keystroke, from
        // any view, which is what the parent-set invariant guarantees.
        let listed: Vec<Command> = catalog_rows(&shared())
            .into_iter()
            .map(|row| row.action)
            .collect();
        assert!(listed.contains(&Entry(EntryCommand::Explore)));
    }
```

Create a new test in `src/tui/palette/command/mod.rs` (add a `#[cfg(test)]
mod tests` block at the end of the file if there is none):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explore_accepts_a_file_or_a_directory_and_advertises_alt_enter() {
        // Both kinds explore fine: a file roots the tree beside it, a
        // directory roots it at itself.
        assert_eq!(
            EntryCommand::Explore.requirement(),
            EntryRequirement::Any
        );
        assert_eq!(EntryCommand::Explore.shortcut(), Some("⌥↵"));
        assert_eq!(
            EntryCommand::Explore.picker_title(),
            "Explore which entry?"
        );
    }
}
```

- [ ] **Step 2: Run the tests and verify they fail**

```sh
cargo test --release palette::command
```

Expected: compile errors, `no variant named 'Explore'` and `cannot find
function 'explore_label'`.

- [ ] **Step 3: Write the implementation**

In `src/tui/palette/command/mod.rs`, add the variant to `EntryCommand` after
`Reveal`:

```rust
    /// Reveal the entry's directory in Finder. A file resolves to its parent.
    Reveal,
    /// Switch the brain-directory view to its tree sub-view, rooted at the
    /// current search scope and opened on this entry.
    Explore,
```

Add it to `requirement`:

```rust
            Self::Open | Self::Reveal | Self::Explore | Self::CopyDirPath | Self::Delete => {
                EntryRequirement::Any
            }
```

Add it to `shortcut`:

```rust
            Self::Explore => Some("⌥↵"),
```

Add it to `picker_title`:

```rust
            Self::Explore => "Explore which entry?",
```

In `src/tui/palette/command/labels.rs`, add after `open_dir_label`:

```rust
/// The "Explore" row label for a given entry name, elided with the same
/// threshold as the other contextual filename rows.
#[must_use]
pub(crate) fn explore_label(name: &str) -> String {
    format!(
        "Explore '{}'",
        truncate_label_filename(name, LABEL_MAX_FILENAME)
    )
}
```

In `src/tui/palette/command/naming.rs`, add `explore_label` to the `use
super::labels::{…}` import list, then add to `named_entry_label`:

```rust
        EntryCommand::Explore => explore_label(&target.filename),
```

and to `generic_entry_label`:

```rust
        EntryCommand::Explore => "Explore a file or directory in the tree",
```

In `src/tui/palette/command/catalog.rs`, add to `COMMANDS` in the brain
directory block, directly after `Entry(EntryCommand::Reveal)`:

```rust
    Entry(EntryCommand::Reveal),
    Entry(EntryCommand::Explore),
```

- [ ] **Step 4: Run the tests and verify they pass**

```sh
cargo test --release palette::command
```

Expected: PASS. The pre-existing `every_declared_command_is_listed_with_no_target_in_context`
test now also covers `Explore`.

- [ ] **Step 5: Commit**

```sh
cargo clippy --release --all-targets -- -D warnings
git add src/tui/palette
git commit -m "feat: declare the Explore entry command

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: `SearchEffect` variants and `Alt+Enter` in the search sub-view

**Files:**
- Modify: `src/tui/state/shell.rs`

- [ ] **Step 1: Write the failing test**

`src/tui/state/shell.rs` already has a `#[cfg(test)] mod tests` block near the
bottom (it contains `handle_search_input` cases around line 354). Add:

```rust
    #[test]
    fn alt_enter_explores_the_highlighted_entry() {
        let mut state = shell_state_with_entries();

        assert_eq!(
            state.handle_search_input(KeyCode::Enter, false, true),
            SearchEffect::Explore(PathBuf::from("/brain/projects/plan.md"))
        );
    }

    #[test]
    fn alt_enter_with_nothing_highlighted_does_nothing() {
        let mut state = ShellState::new(crate::picker::App::new(&[], ""), PanelSide::DEFAULT);

        assert_eq!(
            state.handle_search_input(KeyCode::Enter, false, true),
            SearchEffect::None
        );
    }

    #[test]
    fn plain_and_ctrl_enter_keep_their_meanings() {
        let mut state = shell_state_with_entries();

        assert_eq!(
            state.handle_search_input(KeyCode::Enter, false, false),
            SearchEffect::Open(PathBuf::from("/brain/projects/plan.md"))
        );
        assert_eq!(
            state.handle_search_input(KeyCode::Enter, true, false),
            SearchEffect::Reveal(PathBuf::from("/brain/projects/plan.md"))
        );
    }
```

Add this helper inside the same test module (adapt it to whatever fixture the
module already uses for entries; if one exists, use that instead of adding a
second one):

```rust
    fn shell_state_with_entries() -> ShellState {
        let entries = vec![crate::entry::Entry {
            path: PathBuf::from("/brain/projects/plan.md"),
            display: "~/brain/projects/plan.md".to_owned(),
            bucket: crate::entry::Bucket::Projects,
            is_dir: false,
        }];
        ShellState::new(crate::picker::App::new(&entries, ""), PanelSide::DEFAULT)
    }
```

- [ ] **Step 2: Run the tests and verify they fail**

```sh
cargo test --release tui::state::shell
```

Expected: `no variant named 'Explore'` on `SearchEffect`.

- [ ] **Step 3: Write the implementation**

In `src/tui/state/shell.rs`, extend the enum:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SearchEffect {
    None,
    Quit,
    Open(PathBuf),
    Reveal(PathBuf),
    OpenPalette,
    ConfirmPdf(PathBuf),
    Refresh,
    ConfirmDelete(PathBuf),
    /// Switch to the tree sub-view, rooted at the current scope and opened on
    /// this path.
    Explore(PathBuf),
    /// Leave the tree sub-view for the search sub-view.
    BackToSearch,
    /// Move the tree's root to this directory and rebuild.
    Reroot(PathBuf),
}
```

In `handle_search_input`, replace the `KeyCode::Enter` arm:

```rust
            KeyCode::Enter => self
                .search
                .selected_path()
                .map_or(SearchEffect::None, |path| {
                    if alt {
                        SearchEffect::Explore(path)
                    } else if ctrl {
                        SearchEffect::Reveal(path)
                    } else {
                        SearchEffect::Open(path)
                    }
                }),
```

- [ ] **Step 4: Run the tests and verify they pass**

```sh
cargo test --release tui::state::shell
```

Expected: PASS. Other match sites on `SearchEffect` may now fail to compile
because the enum is non-exhaustive there; `apply_search_view_effect` in
`src/tui/search_view.rs` is the main one. Add temporary arms so the build
stays green; Task 9 replaces them with real behavior:

```rust
        SearchEffect::Explore(_) | SearchEffect::BackToSearch | SearchEffect::Reroot(_) => {}
```

- [ ] **Step 5: Commit**

```sh
cargo test --release && cargo clippy --release --all-targets -- -D warnings
git add src/tui
git commit -m "feat: bind Alt+Enter to explore the highlighted entry

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: `src/tree/input.rs` — the tree's key decisions

**Files:**
- Modify: `src/tree/input.rs` (replacing the placeholder)

- [ ] **Step 1: Write the failing tests**

Replace `src/tree/input.rs` with:

```rust
//! The tree sub-view's key decisions.
//!
//! Pure: every arm either moves the widget's own state or names an effect for
//! the shell to run. Nothing here touches the filesystem or the terminal.

use std::path::Path;

use crossterm::event::KeyCode;

use crate::tui::state::SearchEffect;

use super::TreeView;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{Bucket, Entry};
    use std::path::PathBuf;

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
        // The one case where Enter does not mean "act on this entry": the
        // synthetic ../ row is navigation, not a file.
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
```

- [ ] **Step 2: Run the tests and verify they fail**

```sh
cargo test --release tree::input
```

Expected: `cannot find function 'handle_tree_input'`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` block in `src/tree/input.rs`:

```rust
/// Route one keystroke in the tree sub-view.
///
/// Movement and expansion mutate the widget's state in place and produce
/// [`SearchEffect::None`]; everything that *acts* names the same effect the
/// search sub-view would, so both sub-views run through one applier.
pub(crate) fn handle_tree_input(
    view: &mut TreeView,
    code: KeyCode,
    ctrl: bool,
    alt: bool,
) -> SearchEffect {
    match code {
        KeyCode::Char('c') if ctrl => SearchEffect::Quit,
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

/// How far a page key moves. Matches the search picker's `PAGE_SIZE` so the
/// two sub-views scroll at the same rate.
const PAGE: usize = 10;

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

/// Apply a movement and report that nothing else needs to happen. Navigation
/// is not a command, which is why it produces no effect.
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
```

The module's imports are then:

```rust
use std::path::PathBuf;

use crossterm::event::KeyCode;

use crate::tui::state::SearchEffect;

use super::TreeView;
```

Note `use std::path::Path;` is **not** among them: nothing in this module
names `Path` directly. Drop it from the header you wrote in Step 1, and drop
the `use std::path::PathBuf;` from the test module since the parent now
provides it.

- [ ] **Step 4: Run the tests and verify they pass**

```sh
cargo test --release tree::input
```

Expected: 10 passing tests.

- [ ] **Step 5: Commit**

```sh
cargo clippy --release --all-targets -- -D warnings
git add src/tree
git commit -m "feat: route the tree sub-view's keys

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: Wire the sub-view into the shell

**Files:**
- Modify: `src/tui/state/shell.rs`
- Create: `src/tui/tree_view.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/search_view.rs`
- Modify: `src/tui/app_actions/entry_commands.rs`
- Modify: `src/tui/event_loop/run.rs`
- Modify: `src/tui/draw/mod.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `src/tui/state/shell.rs`:

```rust
    #[test]
    fn the_brain_directory_starts_on_search_and_toggles_to_the_tree() {
        let mut state = shell_state_with_entries();
        assert_eq!(state.brain_dir_view(), BrainDirView::Search);

        state.show_tree(
            &[],
            std::path::Path::new("/brain"),
            std::path::Path::new("/brain/projects/plan.md"),
        );
        assert_eq!(state.brain_dir_view(), BrainDirView::Tree);

        state.show_search();
        assert_eq!(state.brain_dir_view(), BrainDirView::Search);
    }

    #[test]
    fn the_tree_sub_view_does_not_add_a_main_view() {
        // The tree replaces the search panel in the same slot; Ctrl+L / Ctrl+H
        // must keep cycling exactly three main views.
        assert_eq!(crate::main_view::MainView::CYCLE.len(), 3);
    }
```

- [ ] **Step 2: Run the tests and verify they fail**

```sh
cargo test --release tui::state::shell
```

Expected: `cannot find type 'BrainDirView'`.

- [ ] **Step 3: Add the axis to `ShellState`**

In `src/tui/state/shell.rs`:

```rust
/// Which sub-view the brain-directory main view is showing.
///
/// This is an axis *inside* one main view, like the tasks view's `View`, not a
/// fourth entry in `MainView::CYCLE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrainDirView {
    Search,
    Tree,
}
```

Add the fields to the struct, and initialise them in `new`. `ShellState::new`
does not currently take the brain root, so add it as a parameter:

```rust
pub(crate) struct ShellState {
    // … existing fields …
    brain_dir_view: BrainDirView,
    tree: crate::tree::TreeView,
}

impl ShellState {
    pub(crate) fn new(
        search: crate::picker::App,
        panel_side: PanelSide,
        brain_root: &std::path::Path,
    ) -> Self {
        Self {
            main_view: MainView::Tasks,
            focus: Panel::Tasks,
            panel_side,
            brain_rect: None,
            search,
            logs_view: None,
            active_brain_tab: BrainTab::Main,
            quit_requested: false,
            brain_dir_view: BrainDirView::Search,
            tree: crate::tree::TreeView::empty(brain_root),
        }
    }
```

Update every `ShellState::new` call site (production and tests) to pass the
root. `cargo test --release 2>&1 | grep "ShellState::new"` finds them.

Add the accessors:

```rust
    pub(crate) const fn brain_dir_view(&self) -> BrainDirView {
        self.brain_dir_view
    }

    /// Open the tree on `target` and show it.
    pub(crate) fn show_tree(&mut self, entries: &[Entry], brain_root: &Path, target: &Path) {
        self.tree = crate::tree::TreeView::explore(entries, brain_root, target);
        self.brain_dir_view = BrainDirView::Tree;
    }

    pub(crate) const fn show_search(&mut self) {
        self.brain_dir_view = BrainDirView::Search;
    }

    pub(crate) fn reroot_tree(&mut self, entries: &[Entry], brain_root: &Path, root: &Path) {
        self.tree.reroot(entries, brain_root, root);
    }

    pub(crate) fn handle_tree_input(
        &mut self,
        code: KeyCode,
        ctrl: bool,
        alt: bool,
    ) -> SearchEffect {
        crate::tree::input::handle_tree_input(&mut self.tree, code, ctrl, alt)
    }

    pub(crate) fn render_tree(&mut self, frame: &mut Frame, area: Rect) {
        crate::tree::view::draw_into(frame, &mut self.tree, area);
    }

    /// The tree's selected entry as palette context, so the palette's
    /// contextual rows name what the tree is pointing at.
    pub(crate) fn selected_tree_path(&self) -> Option<PathBuf> {
        self.tree.selected_path()
    }
```

Add `use std::path::Path;` and `use crate::entry::Entry;` if not already
imported (`Entry` is).

- [ ] **Step 4: Run the tests and verify they pass**

```sh
cargo test --release tui::state::shell
```

Expected: PASS. `render_tree` will not compile until Task 10; comment it out
for now, or land Task 10 first if you prefer.

- [ ] **Step 5: Create the glue module**

Create `src/tui/tree_view.rs`:

```rust
//! The brain-directory tree sub-view's glue.
//!
//! Mirrors `search_view`: the pure decision lives in `tree::input`, and this
//! module turns the resulting effect into an app action. The two sub-views
//! share one effect enum, so `Open`, `Reveal`, PDF, delete, refresh, palette,
//! and quit run through exactly the same code for both.

use crossterm::event::KeyEvent;

use crate::tui::App;
use crate::tui::state::{SearchEffect, ShellState};

pub(crate) fn handle_tree_view_key(
    shell: &mut ShellState,
    k: &KeyEvent,
    ctrl: bool,
    alt: bool,
) -> SearchEffect {
    shell.handle_tree_input(k.code, ctrl, alt)
}

impl App {
    /// Open the tree on `target`, rooted at the scope the picker's current
    /// entries describe.
    pub(crate) fn explore_entry(&mut self, target: &std::path::Path) {
        let root = self.context.workspace_root().to_path_buf();
        let entries = self.collect_current_scope();
        self.shell.show_tree(&entries, &root, target);
        self.shell.show_main_view(crate::main_view::MainView::BrainSearch);
    }

    /// Move the tree's root and rebuild from a fresh walk of that directory.
    pub(crate) fn reroot_tree(&mut self, root: &std::path::Path) {
        let brain_root = self.context.workspace_root().to_path_buf();
        let entries = crate::entry::collect(
            &brain_root,
            &crate::tui::search_view::all_bucket_roots(&brain_root),
        )
        .unwrap_or_default();
        self.shell.reroot_tree(&entries, &brain_root, root);
    }

    /// The entries the search sub-view currently holds, re-walked so the tree
    /// and the picker agree on what is on disk.
    fn collect_current_scope(&self) -> Vec<crate::entry::Entry> {
        let brain_root = self.context.workspace_root();
        crate::entry::collect(
            brain_root,
            &crate::tui::search_view::all_bucket_roots(brain_root),
        )
        .unwrap_or_default()
    }
}
```

Add `pub(crate) mod tree_view;` to `src/tui/mod.rs` beside `mod search_view;`.

- [ ] **Step 6: Replace the placeholder effect arms**

In `src/tui/search_view.rs`, replace the temporary arm added in Task 7 with
real behavior:

```rust
        SearchEffect::Explore(path) => app.explore_entry(&path),
        SearchEffect::BackToSearch => app.shell.show_search(),
        SearchEffect::Reroot(root) => app.reroot_tree(&root),
```

In `src/tui/app_actions/entry_commands.rs`, add the `Explore` arm to
`run_entry_command`:

```rust
            EntryCommand::Explore => self.explore_entry(path),
```

- [ ] **Step 7: Route keys and rendering**

In `src/tui/event_loop/run.rs`, add the import beside the search one:

```rust
use crate::tui::tree_view::handle_tree_view_key;
```

and replace the `MainView::BrainSearch` arm:

```rust
            MainView::BrainSearch => {
                let effect = match app.shell.brain_dir_view() {
                    BrainDirView::Search => handle_search_view_key(&mut app.shell, k, ctrl, alt),
                    BrainDirView::Tree => handle_tree_view_key(&mut app.shell, k, ctrl, alt),
                };
                apply_search_view_effect(app, effect)
            }
```

importing `BrainDirView` from `crate::tui::state`.

In `src/tui/draw/mod.rs`, replace the `MainView::BrainSearch` arm:

```rust
        MainView::BrainSearch => match app.shell.brain_dir_view() {
            BrainDirView::Search => app.shell.render_search(f, main_area),
            BrainDirView::Tree => app.shell.render_tree(f, main_area),
        },
```

In `src/tui/app_actions/targets.rs`, make the palette's entry context follow
the active sub-view, so a contextual row names what the tree is pointing at:

```rust
    fn entry_context(&self) -> Option<crate::tui::palette::EntryContext> {
        if self.shell.main_view() != MainView::BrainSearch {
            return None;
        }
        match self.shell.brain_dir_view() {
            BrainDirView::Search => self.shell.selected_entry_context(),
            BrainDirView::Tree => self.shell.selected_tree_entry_context(),
        }
    }
```

Add `selected_tree_entry_context` to `ShellState`, building the same
`EntryContext` shape from the tree's selection:

```rust
    /// The tree's selected entry as palette context. The `../` row is not an
    /// entry, so it yields nothing and the palette falls back to its generic
    /// wording plus a target picker.
    pub(crate) fn selected_tree_entry_context(
        &self,
    ) -> Option<crate::tui::palette::EntryContext> {
        let path = self.tree.selected_path()?;
        if self.tree.is_parent_row(&path) {
            return None;
        }
        let is_file = !path.is_dir();
        Some(crate::tui::palette::EntryContext {
            filename: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            dir_reldisplay: String::new(),
            is_file,
            is_markdown: is_file && crate::open_target::is_markdown(&path),
        })
    }
```

- [ ] **Step 8: Run the full suite**

```sh
cargo test --release && cargo clippy --release --all-targets -- -D warnings
```

Expected: all green.

- [ ] **Step 9: Commit**

```sh
git add -A
git commit -m "feat: add the brain-directory tree sub-view axis

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 10: `src/tree/view.rs` — rendering

**Files:**
- Modify: `src/tree/view.rs` (replacing the placeholder)

- [ ] **Step 1: Write the failing test**

Replace `src/tree/view.rs` with:

```rust
//! Rendering the tree panel: header / separator / tree / footer, in the same
//! bordered sub-rect the search panel uses so the two sub-views are visually
//! interchangeable.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::Paragraph,
};
use tui_tree_widget::Tree;

use crate::render;

use super::TreeView;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
}
```

- [ ] **Step 2: Run the tests and verify they fail**

```sh
cargo test --release tree::view
```

Expected: `cannot find function 'header_text'`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` block:

```rust
/// Render the tree panel into `area`.
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

    // `Tree::new` only errors on duplicate identifiers among the items, which
    // cannot happen: every identifier is a distinct absolute path.
    if let Ok(tree) = Tree::new(view.items()) {
        let tree = tree
            .highlight_symbol("▏")
            .node_closed_symbol("▸ ")
            .node_open_symbol("▾ ")
            .node_no_children_symbol("  ");
        f.render_stateful_widget(tree, chunks[2], view.state_mut());
    }

    f.render_widget(Paragraph::new(render::tree_footer_line()), chunks[3]);
}

/// What the header says the tree is showing: the sub-view name and the root
/// as a brain-relative path, or `all` at the brain root itself.
fn header_text(root: &std::path::Path, brain_root: &std::path::Path) -> String {
    let relative = root
        .strip_prefix(brain_root)
        .ok()
        .map(|rel| rel.display().to_string())
        .filter(|rel| !rel.is_empty())
        .unwrap_or_else(|| "all".to_owned());
    format!("tree · {relative}")
}
```

`TreeView` needs a `brain_root` to render the header. Add the field and
accessor in `src/tree/mod.rs`: store `brain_root: PathBuf` in `empty` (it
already receives it) and keep it through `rebuild`:

```rust
    pub(crate) fn brain_root(&self) -> &Path {
        &self.brain_root
    }
```

- [ ] **Step 3b: Add the two render helpers**

The search view's `render::header_line` renders a `"{matched} of {total}"`
tail and `render::footer_line` reads `"type filter"`. Neither is true of the
tree, which has no query, so it gets its own pair. All color stays inside
`src/render.rs` per the house rule.

Add to `src/render.rs`, beside the existing helpers:

```rust
/// The tree sub-view's header: ` BRAIN · tree · projects · 42 items`.
#[must_use]
pub fn tree_header_line(scope: &str, count: usize) -> Line<'static> {
    let title = Style::new().fg(ACCENT_PURPLE).add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::raw(" "),
        Span::styled("BRAIN", title),
        sep_span(),
        Span::styled(scope.to_owned(), primary_bold()),
        sep_span(),
        Span::styled(
            format!("{count} {}", if count == 1 { "item" } else { "items" }),
            dim(),
        ),
    ])
}

/// The tree sub-view's footer. It names navigation rather than filtering,
/// because the tree has no query line.
#[must_use]
pub fn tree_footer_line() -> Line<'static> {
    let key = primary_bold();
    let lbl = dim();
    let dot = very_dim();
    Line::from(vec![
        Span::raw(" "),
        Span::styled("→←", key),
        Span::styled(" expand", lbl),
        Span::styled("   ", dot),
        Span::styled("↵", key),
        Span::styled(" open", lbl),
        Span::styled("   ", dot),
        Span::styled("^↵", key),
        Span::styled(" reveal", lbl),
        Span::styled("   ", dot),
        Span::styled("esc", key),
        Span::styled(" search", lbl),
    ])
}
```

Cover the pluralisation with a test in `src/render.rs`'s test module:

```rust
    #[test]
    fn the_tree_header_pluralises_its_item_count() {
        let one = tree_header_line("tree · projects", 1);
        assert!(
            one.spans.iter().any(|span| span.content == "1 item"),
            "{one:?}"
        );
        let many = tree_header_line("tree · projects", 42);
        assert!(
            many.spans.iter().any(|span| span.content == "42 items"),
            "{many:?}"
        );
    }
```

If `src/render.rs` has no `#[cfg(test)] mod tests` block, add one.

- [ ] **Step 4: Run the tests and verify they pass**

```sh
cargo test --release tree::view
```

Expected: 3 passing tests.

- [ ] **Step 5: Verify it actually renders**

```sh
cargo build --release
./target/release/brain
```

Press `Ctrl+B` for the brain directory, highlight an entry, press `Alt+Enter`.
Confirm: the tree appears, `→` / `←` expand and collapse, `Enter` opens,
`Esc` returns to search, and at a scoped root the `../` row re-roots. Then
`Ctrl+P` and confirm the `Explore …` row is listed.

- [ ] **Step 6: Commit**

```sh
cargo clippy --release --all-targets -- -D warnings
git add src/tree
git commit -m "feat: render the brain-directory tree panel

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 11: Shortcuts table, docs, and the version bump

The repository's docs contract requires these in the same change as the
feature. Two guard tests in `src/tasks/shortcuts/tests.rs` pin exact lists and
will fail until they are updated deliberately, which is the point of them.

**Files:**
- Modify: `src/tasks/shortcuts/table.rs`
- Modify: `src/tasks/shortcuts/tests.rs`
- Modify: `docs/glossary.md`, `docs/features.md`, `docs/keybindings.md`, `docs/architecture.md`, `docs/data-model.md`, `docs/decisions.md`
- Modify: `Cargo.toml`, `Cargo.lock`

- [ ] **Step 1: Update the two guard tests first (they are the RED)**

In `src/tasks/shortcuts/tests.rs`, extend the brain-directory key list:

```rust
#[test]
fn help_lists_the_brain_directory_bindings() {
    let rows = in_group(Group::BrainDirectory);
    let keys: Vec<&str> = rows.iter().map(|s| s.keys).collect();
    assert_eq!(keys, ["↵", "^↵", "⌥↵", "→ / ← / Space", "^G", "^D", "^R"]);
}
```

and the exempt list:

```rust
    assert_eq!(
        exempt,
        [
            "j / k",
            "d / u",
            "PgDn / PgUp",
            "g / G",
            "Alt+U / Alt+D",
            "Esc",
            "^P",
            "→ / ← / Space",
        ]
    );
```

- [ ] **Step 2: Run them and verify they fail**

```sh
cargo test --release shortcuts
```

Expected: both assertions fail, showing the current lists.

- [ ] **Step 3: Add the rows**

In `src/tasks/shortcuts/table.rs`, in the `// --- Brain directory ---` block,
after the `^↵` row:

```rust
    Shortcut {
        keys: "⌥↵",
        label: "explore",
        desc: "Explore the highlighted entry in the directory tree. ⌥↵ again, or Esc, returns to search. Bound to Alt rather than Shift because the kitty keyboard protocol exempts Enter from modifier reporting, so Shift+Enter is byte-identical to Enter",
        group: Group::BrainDirectory,
        in_footer: false,
        commands: &[Entry(EntryCommand::Explore)],
    },
    Shortcut {
        keys: "→ / ← / Space",
        label: "expand",
        desc: "Expand / collapse / toggle the selected tree node. Pure navigation, so it runs no command",
        group: Group::BrainDirectory,
        in_footer: false,
        commands: &[],
    },
```

Also extend the existing `↵` row's description to cover the `../` case:

```rust
        desc: "Open the highlighted entry (text → editor tab, blob → system open, dir → Finder). On the tree's ../ row it re-roots one level up instead, never above the brain root",
```

- [ ] **Step 4: Run and verify green**

```sh
cargo test --release && cargo clippy --release --all-targets -- -D warnings
```

- [ ] **Step 5: Update the docs**

| File | What to add |
| --- | --- |
| `docs/glossary.md` | Widen the **sub-view** row: the tasks view's tabbed modes *and* the brain-directory view's search/tree modes (`tui::state::BrainDirView`) |
| `docs/features.md` | A brain-directory tree section: what it shows, how you enter and leave it, the `../` re-root and its brain-root ceiling, and that it reuses the collected entries so `Ctrl+R` is what refreshes it |
| `docs/keybindings.md` | The tree's bindings in the brain-search section, plus the new `⌥↵`; note that `Esc` in the tree returns to search rather than quitting |
| `docs/architecture.md` | `src/tree/` in the module list (`mod`, `root`, `build`, `input`, `view`) and the `tui-tree-widget` dependency justification, per "don't add dependencies casually" |
| `docs/data-model.md` | `Entry::is_dir`, and the tree model: identifiers are absolute paths, the root is derived from the entries' buckets, the `../` row is a synthetic leaf carrying its target directory |
| `docs/decisions.md` | Four entries: Alt+Enter over Shift+Enter (with the kitty C0 table evidence); the tree is a sub-view, not a fourth main view; contents come from the collected entries rather than lazy `read_dir`; `tui-tree-widget` pinned to 0.23.1 until ratatui 0.30 |

- [ ] **Step 6: Bump the version**

`Cargo.toml` is currently `0.96.0`. This is an additive user-visible feature,
so before v1 that is a minor bump:

```sh
sed -i '' 's/^version = "0.96.0"$/version = "0.97.0"/' Cargo.toml
cargo build --release   # moves Cargo.lock with it
```

- [ ] **Step 7: Final verification**

```sh
cargo test --release
cargo clippy --release --all-targets -- -D warnings
./target/release/brain workspace list
./target/release/brain tasks today --no-tui -b brain
```

Expected: the suite green, clippy silent, and both headless commands working
(the tree sub-view must not have disturbed any non-TUI path).

- [ ] **Step 8: Commit**

```sh
git add -A
git commit -m "docs: document the brain-directory tree sub-view

Covers the docs contract for a new feature, a new keybinding, a new
palette command, an Entry model change, and a new dependency. Bumps to
0.97.0.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

## Self-review notes

Checked against the spec:

| Spec section | Covered by |
| --- | --- |
| Alt+Enter, not Shift+Enter | Tasks 7, 11 |
| A sub-view, not a fourth main view | Task 9, plus the `MainView::CYCLE.len() == 3` guard |
| Contents from the collected entries | Tasks 1, 4, 5 |
| Scope root derived from entries | Task 3 (`scope_root`) |
| `../` re-roots, bounded by the brain root | Tasks 3, 4, 8, 9 |
| Brain root shows directory names | Task 4 (`label_for` uses `file_name`) |
| `tui-tree-widget` 0.23.1 pinned | Task 2 |
| `EntryCommand::Explore` | Task 6 |
| Sub-view axis on `ShellState` | Task 9 |
| Module layout | Tasks 3, 4, 5, 8, 9, 10 |
| Effects | Task 7 |
| Keybindings | Tasks 7, 8, 11 |
| Rendering | Task 10 |
| Testing table | Every task's RED step |
| Documentation | Task 11 |

One thing the implementer should expect rather than treat as a mistake:
`ShellState::new` gains a `brain_root` parameter in Task 9, which ripples to
every call site including tests. That is expected churn.

Verified against the real source while writing this plan, so these are facts
rather than assumptions:

- `render::header_line(scope, total, matched)` and `render::footer_line()`
  exist with those signatures, and both are search-specific, which is why
  Task 10 adds `tree_header_line` / `tree_footer_line` instead of reusing them.
- `TreeItem::new` and `Tree::new` both return `Result` and only error on
  duplicate identifiers, which distinct absolute paths cannot produce.
- `TreeState` exposes `key_up`, `key_down`, `key_left`, `key_right`,
  `toggle_selected`, `select_first`, `select_last`, `open`, `select`,
  `selected`, `opened`, and `scroll_selected_into_view`.
- `src/tasks/shortcuts/tests.rs` pins both the brain-directory key list and
  the no-command exemption list, so Task 11's guard edits are required, not
  optional.
