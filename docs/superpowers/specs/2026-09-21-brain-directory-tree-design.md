# Brain-directory tree sub-view

**Date:** 2026-09-21
**Status:** Approved, not yet implemented
**Branch:** `feat/tree-view`

## Problem

The brain-directory main view is a fuzzy search picker over a flat list of
collected entries. Fuzzy search is the right tool when you know roughly what a
file is called. It is the wrong tool when you want to see how a corner of the
brain is *shaped*: which directories exist under `projects/atlas`, what sits
beside a file, what a bucket contains before you have a name in mind.

There is currently no way to browse the brain as a directory tree from inside
the shell. The only escape hatch is revealing a directory in Finder, which
leaves the terminal.

## Goal

Add a tree sub-view to the brain-directory main view. From the search
sub-view, `Alt+Enter` on the highlighted entry switches to a tree rooted at
the current search scope, pre-expanded along that entry's ancestors with the
entry selected. Arrow keys expand and collapse; `Enter` and `Ctrl+Enter` keep
the meanings they already have in search (open the entry, reveal its
directory). `Esc` or another `Alt+Enter` returns to search.

## Non-goals

- No filter or query inside the tree. The search sub-view is the filter.
- No file mutation beyond what the brain directory already offers (PDF
  creation, trash). The tree is a navigation surface, not a file manager.
- No hidden files. The tree shows exactly what search shows.
- No fourth main view. `MainView::CYCLE` is unchanged.

## Decisions

### Alt+Enter, not Shift+Enter

The feature was first requested as `Shift+Enter`. That keystroke cannot reach
brain. The shell pushes `KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES`
(`src/tui/runtime/terminal.rs`), and the kitty keyboard protocol explicitly
exempts Enter from that flag: "The only exceptions are the Enter, Tab and
Backspace keys which still generate the same bytes as in legacy mode." Its C0
controls table gives Enter the byte `0xd` for no modifiers, Ctrl, Shift, and
Ctrl+Shift alike. Shift+Enter is therefore byte-identical to Enter.

Reporting it would require `REPORT_ALL_KEYS_AS_ESCAPE_CODES`, which also stops
the terminal from sending text at all ("text will not be sent, instead only key
events are sent"). That would put the search query input, the brain panel's
`key_to_bytes` PTY forwarding, and every non-ASCII keystroke at risk for the
sake of one binding.

`Alt+Enter` arrives as `ESC 0x0D` on every terminal with no protocol
negotiation. `src/tui/keymap.rs::enter_inserts_newline` already relies on
exactly this encoding for the brain-input modal's newline, for the same reason.

Observed caveat, recorded so a future reader is not misled by the table above:
iTerm2 3.6.11 *does* report `Ctrl+Enter` distinctly, so the existing
`Ctrl+Enter` reveal binding works there despite the spec. That is terminal
behavior beyond the spec's guarantee, and it is not something to build a new
binding on.

### A sub-view, not a fourth main view

The tree replaces the search panel in the same slot rather than joining the
main-view cycle. `Ctrl+B` still lands on the brain directory, `Ctrl+L` / `Ctrl+H`
still cycle three main views, and `default_tui_view` needs no new value. This
matches the existing precedent: `default_tui_view` accepts `tasks`,
`brain_dir`, and `brain_llm`, and rejects the tasks view's own sub-views, so
sub-views are not a startup surface.

Returning to the brain directory shows whichever sub-view was last active. The
shell is persistent and can stay open for days; snapping back to search would
discard tree state the user built up.

### Contents come from the already-collected entries

The tree is built from the `Vec<Entry>` the picker already holds, not from
fresh directory reads. This means:

- Entering the tree costs no disk I/O and is instant.
- The tree shows exactly what search showed, including `entry::collect`'s
  hidden-file exclusion.
- `Ctrl+R` is what refreshes it, the same key that refreshes search.

Re-rooting via `../` is the single exception that re-walks, because a scoped
search only holds entries beneath its own bucket.

### The scope root is derived from the entries, not stored

`picker::App` has no scope-root field. Scope is implicit in the entries it
holds, each of which carries its `Bucket`. So the tree derives its starting
root purely: entries spanning a single bucket root at that bucket's directory,
and entries spanning more than one (or none) root at the brain root. That keeps
the derivation in one tested function and avoids adding a second source of
truth for the scope that could drift from the entries themselves.

### `../` re-roots, bounded by the brain root

The tree's root starts at the current search scope's root. When that root is
not the brain root, a synthetic `../` row sits at the top of the tree.
Selecting it re-roots to the parent directory and re-walks. At the brain root
the row is absent, so there is no way to navigate above it.

Because the five bucket roots are direct children of the brain root, this is a
single step today. It is written as a general ascend-one-level operation
anyway, so it stays correct if scopes ever nest deeper, and so the rule
"never above the brain root" lives in one tested function rather than in a
special case.

### The brain root shows directory names, not bucket labels

At the brain root the tree's top-level rows are `capture/`, `projects/`,
`areas/`, `resources/`, and `archive/`, not the search view's `Capture` /
`Projects` section headers. This is a directory view, so it names directories.
Nothing else appears at the brain root because `entry::collect` walks only
those five, which is also what makes the "never above the brain root" rule
enforce itself.

### `tui-tree-widget` 0.23.1, pinned

| Candidate | ratatui 0.29 | Verdict |
| --- | --- | --- |
| `tui-tree-widget` 0.23.1 | Yes (`^0.29`) | **Chosen.** `TreeState` exposes semantic methods (`key_up`/`key_down`/`key_left`/`key_right`, `open`/`close`/`toggle`, `selected`, `scroll_selected_into_view`) and no event handling or filesystem access, which matches the repository's pure/impure split. One transitive dependency, `unicode-width`, already present |
| `tui-tree-widget` 0.24.x | No | Requires `ratatui-core ^0.1` + `ratatui-widgets ^0.3`, the ratatui 0.30 split. Revisit when brain moves to 0.30 |
| `ratatui-explorer` 0.2.0 | Nominally, but wants `crossterm ^0.29` against our pinned 0.28 | Rejected. It is a single-directory `cd` in/out list rather than an expandable tree, and it owns its own filesystem reads, input handling, and theme, duplicating `entry`, `open_target`, and `keymap` |
| Hand-rolled | n/a | Rejected, but viable. Roughly 150 lines of flatten-and-scroll logic the widget already provides and tests |

`TreeState<PathBuf>` satisfies the `Clone + PartialEq + Eq + Hash` identifier
bound, and `selected() -> &[PathBuf]` yields the selected node's path directly,
so the path flows straight into the existing `run_entry_command`.

## Design

### One new command, and the palette work is done

The palette is the parent set of every command, so adding a single
`EntryCommand::Explore` variant produces the palette row in every view
automatically, with contextual wording and a target picker when no entry is in
context.

| Aspect | Value |
| --- | --- |
| `requirement()` | `EntryRequirement::Any` |
| `shortcut()` | `Some("⌥↵")` |
| `picker_title()` | `"Explore which entry?"` |
| Named label | `Explore 'atlas'` |
| Generic label | `Explore a file or directory` |
| Catalog position | Brain-directory group, after `Entry(EntryCommand::Reveal)` |

Run with an entry in context, it switches to the tree rooted at the current
scope, selected at that entry. Run without one (from the tasks view, say), the
existing `EntryTargetPicker` asks which entry first, then does the same.

### Sub-view axis

```rust
// src/tui/state/shell.rs
pub(crate) enum BrainDirView { Search, Tree }

pub(crate) struct ShellState {
    // …
    brain_dir_view: BrainDirView,
    tree: crate::tree::TreeView,
}
```

### Modules

`src/tree/` mirrors the shape of `src/picker/`: a pure model with a thin glue
layer in `src/tui/`.

| File | Responsibility |
| --- | --- |
| `src/tree/mod.rs` | `TreeView { root, state: TreeState<PathBuf>, items }` and its accessors |
| `src/tree/build.rs` | `Vec<Entry>` + root to `Vec<TreeItem<PathBuf>>`, plus the opened-set for pre-expansion along a target's ancestors |
| `src/tree/root.rs` | `scope_root(entries, brain_root)`, `ascend(root, brain_root)`, `shows_parent_row(root, brain_root)` |
| `src/tree/input.rs` | `handle_tree_input(&mut TreeView, code, ctrl, alt) -> SearchEffect` |
| `src/tree/view.rs` | `draw_into(f, &mut TreeView, area)` |
| `src/tui/tree_view.rs` | Glue: applies effects, mirrors `search_view.rs` |

### Effects

`SearchEffect` gains three variants and becomes the brain-directory effect
enum covering both sub-views, so `Open` / `Reveal` / `Quit` / `OpenPalette` /
`Refresh` / `ConfirmPdf` / `ConfirmDelete` plumbing is shared rather than
duplicated into a parallel `TreeEffect`.

| New variant | Meaning |
| --- | --- |
| `Explore(PathBuf)` | Search to tree, rooted at scope, selected at this path |
| `BackToSearch` | Tree to search |
| `Reroot(PathBuf)` | Re-walk under this parent and rebuild the tree |

### Keybindings in the tree sub-view

| Key | Action | `commands` for the parity guard |
| --- | --- | --- |
| `↑` / `↓`, `Ctrl+K` / `Ctrl+J` | Move the selection | empty (navigation) |
| `PgUp` / `PgDn`, `Home` / `End` | Page, first, last | empty (navigation) |
| `→` / `←` | Expand / collapse | empty (navigation) |
| `Space` | Toggle the selected node | empty (navigation) |
| `Enter` | Open the entry | `Entry(EntryCommand::Open)` |
| `Enter` on `../` | Re-root to the parent | empty (navigation) |
| `Ctrl+Enter` | Reveal the entry's directory | `Entry(EntryCommand::Reveal)` |
| `Ctrl+G` | Create a PDF from a markdown file | `Entry(EntryCommand::CreatePdf)` |
| `Ctrl+D` | Delete the entry | `Entry(EntryCommand::Delete)` |
| `Ctrl+R` | Re-walk the brain directory | `Global(GlobalAction::RefreshBrainDirectory)` |
| `Alt+Enter` | Toggle search ↔ tree | `Entry(EntryCommand::Explore)` |
| `Esc` | Back to search | empty (navigation) |
| `Ctrl+C` | Quit | `Global(GlobalAction::Quit)` |
| `Ctrl+P` | Open the command palette | empty (opens the palette) |

`Alt+Enter` is also added to the search sub-view, where it emits
`Explore(path)` for the highlighted entry and does nothing when nothing is
highlighted.

Printable characters do nothing in the tree. There is no query line.

### Rendering

`src/tree/view.rs` renders header, separator, tree, footer into the same
bordered sub-rect the search panel uses, so the two sub-views are visually
interchangeable. The header names the sub-view and the current root
(`Brain directory · tree · projects`). Colors go through `Theme` tokens.

## Testing

Every decision is pushed into a pure function and tested there. No terminal or
filesystem mocking.

| Area | Cases |
| --- | --- |
| `tree::root` | `scope_root` returns the bucket directory when entries span one bucket, the brain root when they span several, and the brain root when there are none; `ascend` returns the parent below the brain root; returns `None` at the brain root; returns `None` for a path outside the brain root; `shows_parent_row` agrees with `ascend` |
| `tree::build` | Entries nest by path depth; a directory precedes its children; the opened-set covers exactly the target's ancestors; the `../` row is present only off-root and carries the parent as its identifier; the brain root lists the five bucket directories by name |
| `tree::input` | One case per row of the keybinding table, including `Enter` on `../` emitting `Reroot` rather than `Open`, and printable characters being inert |
| `EntryCommand::Explore` | `requirement`, `shortcut`, `picker_title`, named and generic labels |
| Catalog | `Explore` is listed; the existing "every declared command is listed" guard covers it |
| Shortcuts table | The parity guard proves each new acting binding names a listed command |
| Sub-view axis | `Explore` switches to the tree; `Esc` and `Alt+Enter` return to search; `MainView::CYCLE` is unchanged |

## Documentation

Per the `docs/` contract, in the same change:

| File | Change |
| --- | --- |
| `docs/glossary.md` | "sub-view" gains the brain-directory axis alongside the tasks one |
| `docs/features.md` | The tree sub-view and what it does |
| `docs/keybindings.md` | The tree's bindings and the new `Alt+Enter` |
| `docs/architecture.md` | The `src/tree/` module list and the `tui-tree-widget` dependency justification |
| `docs/data-model.md` | The tree model and its relationship to `Entry` |
| `docs/decisions.md` | Alt+Enter over Shift+Enter; eager-from-entries; the 0.23.1 pin; `../` bounded at the brain root |
| `src/tasks/shortcuts/table.rs` | The new rows with their `commands` fields |
| `Cargo.toml` + `Cargo.lock` | 0.96.0 to 0.97.0, an additive user-visible feature |

## Risks

| Risk | Mitigation |
| --- | --- |
| `tui-tree-widget` 0.23.1 goes unmaintained while brain stays on ratatui 0.29 | The widget is small and the hand-rolled fallback stays viable; `TreeState`'s surface is the only thing we depend on |
| A large brain makes tree construction slow | Construction is over an in-memory `Vec` the picker already walked, so it is bounded by what search already costs |
| The sub-view axis leaks into main-view logic | The axis lives on `ShellState` and is read only by the brain-directory draw and key paths; a test pins `MainView::CYCLE` |
