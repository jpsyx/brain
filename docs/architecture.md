# Architecture

`brain` is a small Rust CLI that browses `~/brain` (a PARA-organized second
brain: projects / areas / resources) and acts as the single terminal
entry point for the user's knowledge and task workflows.

As of the tasks↔brain merge, `brain` is the single CLI for both the second
brain and the task system; the standalone `tasks` binary is gone. It has two
faces:

- **Bare `brain`** (and `brain tasks …`) opens a **persistent shell**
  (`tui/`) with **two main views** — the **tasks view** (task management,
  agenda, triage; the startup default) and the **brain-directory search view**
  (fuzzy-pick over `~/brain`) — plus one app-level **brain panel** (an
  interactive `claude` session in a PTY). You switch main views with
  `Ctrl+L`/`Ctrl+H` (cycle) or `Ctrl+T`/`Ctrl+B` (jump); the brain panel
  persists across a switch and closing it makes the main view full-width. The
  process owns the terminal until you quit and keeps a little SQLite state so
  it resumes the right Claude session and remembers the panel layout. See
  [glossary.md](glossary.md) for the main-view / sub-view / panel vocabulary.
- **The one-shot subcommands** (`brain pr|ar|re|s|cd|msg`, and
  `brain <freeform>`) are short-lived: they decide *what* should happen, print
  a tiny **plan** to stdout, and the zsh wrapper executes the parent-shell
  effects. `brain tasks {complete|doctor|search|--no-tui …}` are tasks
  utilities (mutations / health-check / plain output) that also stay
  short-lived.

Effects that must happen in the *parent shell* (changing directory, running
the `tasks` zsh function) are never performed by the binary; it emits the
plan and the wrapper executes it. The persistent shell emits a plan only on
exit, and only for the two palette actions that deliberately leave brain
("Go to root" → `cd=`, "Open tasks" → `tasks=1`).

## Two halves: the binary and the wrapper

```
user types `brain …`
  └─→ brain() zsh function (file: ./brain)
       ├─ rebuilds the binary if any src/*.rs is newer (cargo build --release)
       ├─ runs target/release/brain "$@", capturing STDOUT into `plan`
       │    (the TUI renders to /dev/tty, so stdout stays clean)
       └─ parses `plan` line-by-line and applies shell-side effects:
            cd=<path>    → cd
            open=<path>  → open
            edit=<path>  → $VISUAL/$EDITOR/vi
            claude=<msg> → the `cl` alias (claude)
            tasks=1      → the `tasks` zsh function
            anything else → printed verbatim (clap help, errors)
```

The binary never `cd`s, never calls `cl`, never runs `tasks`. Those need
the parent shell. See [decisions.md](decisions.md) for *why*, and
[integrations.md](integrations.md) for the full wire protocol.

## High-level data flow (inside the binary)

```
argv
 └─→ Cli::parse                          (cli.rs)
      └─→ paths::brain_root              (config.json → root, else $HOME/brain)
           ├─ no subcommand, no args ─→ tasks_launch(default)  → tui::run_tui (MERGED SHELL)
           ├─ no subcommand + args   ─→ search(all buckets)    (one-shot picker)
           ├─ Cmd::Pr/Ar/Re          ─→ bucket(): cd (no query) | search (query)
           ├─ Cmd::S                 ─→ search(all buckets)    (one-shot picker)
           ├─ Cmd::Cd                ─→ plan::cd(root)
           ├─ Cmd::Msg               ─→ plan::claude(root, msg)
           └─ Cmd::Tasks(rest)       ─→ TasksCli::parse_from(rest) → tasks_launch:
                                          complete → complete::run (exec mark_done.py)
                                          doctor   → doctor::run_doctor
                                          --no-tui → plain::print_plain
                                          else     → tui::run_tui (MERGED SHELL, tasks view)

brain_shell(root, initial_query)            (the persistent default)
 ├─→ entry::collect(brain, all buckets)     (initial global search set)
 └─→ tui::run(root, entries, query) → Exit  (two panels: search + claude PTY)
       ├─ state::Db: reap dead locks, pick_resume / claim or register_fresh
       ├─ session::build_claude_command + env_for → PtyPane spawns `claude`
       ├─ Alt+H/L switch focus; palette (Ctrl-p) reuses menu::MenuApp
       ├─ Enter on a file opens it WITHOUT quitting (open_target spawners)
       └─ Exit::{Quit | Cd(root) | Tasks} → main emits the matching plan

search(roots, query)                         (one-shot subcommand path)
 ├─→ entry::collect(brain, roots)            (walkdir + hidden filter → Vec<Entry>)
 ├─→ picker::run(entries, query)             (ratatui fuzzy picker → Outcome)
 │     └─ Ctrl-p opens menu::MenuApp as a modal overlay *inside* the loop
 ├─→ Outcome::Selected(Reveal) → open_in_finder
 ├─→ Outcome::Selected(Open)   → open_directly  (is_textlike → edit= vs open=)
 └─→ Outcome::Choice(choice)   → dispatch(Choice)
```

The two paths share the pure picker logic (`picker::App` matching /
navigation, rendered via `picker::draw_into`) and the `menu` palette. The
difference is the shell around them: the one-shot picker owns the whole
screen and exits on selection; `tui` embeds the same search panel in a
bordered half alongside the live brain panel and stays up.

## Modules

### `main.rs`
Owns argv → `Cli` and the top-level `match` over `Cmd`. Bare `brain`
(with or without an initial query) calls `brain_shell()`, which collects the
global entry set and runs `tui::run`, then maps the returned `tui::Exit`
onto a plan (`Cd` → `plan::cd`, `Tasks` → `plan::tasks`, `Quit` → nothing).
The one-shot subcommands keep the old flow: `dispatch()` maps each palette
`Choice` to the same primitives, `search()` is the shared pipeline (collect
→ pick → act), and `open_in_finder` / `open_directly` translate a picked
path into plan directives.

### `cli.rs`
The clap derive surface. `Cli` has free-form `args` plus an optional
`Cmd`. `Cmd` covers `Pr` / `Ar` / `Re` (each with visible aliases),
`S` (search), `Cd`, `Msg`, and `Tasks`. `QueryArgs` is a `Vec<String>`
joined with spaces into the initial picker query / claude message.

### `paths.rs`
Brain-root resolution. `brain_root()` reads `config.json`'s `root`
(tilde-expanded) or falls back to `$HOME/brain`, erroring if the result
isn't a directory. The IO-free pieces (`parse_config_root`,
`expand_tilde_with_home`) are split out so they're unit-testable without a
real `$HOME` or config file. See [config.md](config.md).

### `entry.rs`
`Bucket` (Projects / Areas / Resources / Archive; declaration order =
display order, Archive last) and `Entry` (absolute `path`, `~/brain/...`
`display`, `bucket`).
`collect()` walks each root with `walkdir`, skips hidden files
(`.`-prefixed) and the root itself, and tags every entry with its bucket.
Missing roots are silently skipped.

### `picker.rs`
The ratatui fuzzy picker. `App` **owns** its entries (so the persistent
shell can `set_entries` to rescope a bucket in place), precomputed
`HaystackBuf`s, the query, the current matches, and the interleaved
header/match `display_rows`. `refilter()` runs nucleo substring matching,
sorts matches by bucket then score then walk order, and rebuilds the
section-grouped rows. Navigation (`move_up`/`down`, `page_*`,
`ensure_visible`) keeps the cursor and its section header on screen.
Rendering is delegated to `render.rs` and exposed as `draw_into(f, app,
area)` so both the full-screen one-shot picker and `tui`'s embedded panel
share it. `App` also holds an optional `palette: Option<menu::MenuApp>`
overlay. The one-shot `run()` returns an `Outcome` — `Selected(Open)` on
Enter (a directory match falls back to reveal), `Selected(Reveal)` on
Ctrl-Enter, `Choice` when a palette row is confirmed, or `CreatePdf(path)`
when the markdown→PDF action is confirmed (`Ctrl-G` modal or the palette
row) — or `None` on Esc / Ctrl-c. `App` also owns an optional
`confirm: Option<confirm::Confirm>` overlay (routed before the palette) that
serves both the "Create PDF" and "Delete" confirmations. **Delete** never
becomes an `Outcome`: the one-shot picker trashes in place on `Accept` and
`drop_path`s the entry (`reload_entries` keeps the query), staying open.
In `tui`, the same `App` is driven key-by-key (Enter opens in place
rather than quitting).

### `menu.rs`
The command palette (the top-level menu). It has **no screen of its own**:
the host opens it with `Ctrl-p`, drives its pure `MenuApp` + `handle_key`,
and paints it with `draw_modal` as a centered overlay. `Choice` enumerates
the rows; the row list is built per-open by `items(side, include_msg,
pdf_target, delete_target)` because the rows are contextual: the **layout
toggle** has a dynamic label (`layout_choice_label`: "Move brain panel to the
left" / "...right"), the **"Create PDF for '…'"** row (label via
`create_pdf_label`, which elides a long filename) leads the list only when
`pdf_target` is a highlighted `.md` filename, the **"Delete '…'"** row (label
via `delete_label`, which shares `create_pdf_label`'s ellipsis threshold via
`truncate_label_filename`/`LABEL_MAX_FILENAME`) **trails** the list when
`delete_target` is a highlighted entry of any kind (trailing, so a destructive
action is never the default-selected row), and the "Message brain" row is
dropped when `include_msg` is false (the persistent
shell hides it while the panel is open, shows it to re-open once closed).
`MenuApp::new(side, include_msg, pdf_target, delete_target)` owns the filtered
view; `filter_indices` /
`item_matches` are **pure** matchers (each row's matchable text includes its
1-based number), and `handle_key` is a **pure** key handler (returns
`Continue`/`Confirm`/`Cancel`). `Cancel` (Esc) tells the host to drop the
overlay, not to exit. The same `Choice` can mean different things by context:
in the persistent shell `Msg` opens/focuses the brain panel and
`ToggleLayout` swaps sides; in the one-shot picker `Msg` launches claude via
a plan and `ToggleLayout` is a no-op.

### `confirm.rs`
The shared yes/no confirmation modal. Like `menu`, it has **no screen of its
own**: the picker holds a `Confirm { path, kind, yes }` in its state, the host
drives its pure `handle_key` (returns `Continue`/`Cancel`/`Accept`), and paints
it with `draw_modal` as a centered overlay. `ConfirmKind` selects the flavor:
**Pdf** (green, defaults to Yes; opened by `Ctrl-G` on a `.md` file) and
**Delete** (red, defaults to **No** because it's destructive; opened by
`Ctrl-D` on any entry). The pure `accent`/`title`/`question` helpers key off
`kind`; on `Accept` the host converts (Pdf) or trashes (Delete). The key
handling, the kind-keyed chrome, and the button styling are unit-tested.

### `render.rs`
Pure functions that build styled ratatui `Line`s for the picker (header,
input, separator, section header, entry with coalesced highlight spans,
empty state, footer) plus the Tokyo-Night palette constants. No state,
no IO — every function maps inputs to a `Line`, which is why they're
cheap to unit test.

### `open_target.rs`
Pure decisions about acting on a picked path: `is_textlike` (extension
allowlist; extensionless files count as text) and `finder_target` (a file
reveals its parent dir, a directory reveals itself). It also holds the
new-tab opener: pure builders (`edit_shell_command`,
`iterm_new_tab_applescript`) plus thin impure spawners (`open_in_editor_tab`,
`open_with_system`) the persistent shell uses to open files without tearing
itself down — text → a new iTerm2 tab, everything else → system `open`.
The PDF path lives here too: pure `is_markdown` (strictly `.md`) and
`pdf_output_path` (colocated, same stem, `.pdf`), plus the impure `create_pdf`
(drop any existing same-name PDF, then shell out to the user's
`markdown-to-pdf` script) that the "Create PDF" command calls. The delete path
mirrors this: pure `trash_applescript` (a Finder `delete POSIX file` line,
path escaped) plus the impure `move_to_trash` that shells out to `osascript`,
so the "Delete" command performs a recoverable, user-style trash rather than
an `rm`.

### `main_view.rs`
The app-level main-view axis: the `MainView` enum (`Tasks` / `BrainSearch`),
`MainView::step` cycling, and the pure key-classifiers `ctrl_cycles_view`
(`Ctrl+H`/`Ctrl+L`), `ctrl_jumps_view` (`Ctrl+T`/`Ctrl+B`), and
`alt_opens_help` (`Alt+S`). Pure and unit-tested; the merged `tui` App applies
the results.

### `config.rs`
Runtime config loaded from `config.json` (walks up from the exe). Fields:
`daily_triage_name_pattern`, `linear_base_url`, `day_rollover_hour`. Missing
file/fields fall back to defaults. (Root resolution stays in `paths.rs`, which
reads the same file's `root` — serde ignores the fields each doesn't use.)

### `tasks/`
Everything specific to the **tasks main view**, ported from the old `tasks`
crate under one namespace: `task` (CSV model + load), `view` (sub-views +
`build_view`), `selector` (date parsing), `render` (task-card lines, chrome,
markdown), `shortcuts` (the help/footer catalogue), `complete` (exec
`mark_done.py`), `doctor` (health check), `plain` (`--no-tui` printer), and
`cli` (the tasks clap args, nested under `brain tasks`). Reuses the crate-level
`session` / `state` / `pty_pane` / `plan` shared with the brain-search view.

### `tui/` (the merged shell)
The persistent shell, built from the ported tasks `tui/` and extended with the
main-view axis. One `App` owns: the tasks-view state, the embedded
`picker::App` (`search`, the brain-directory view), the app-level `brain`
panel, `focus` (main panel vs brain panel), `main_view`, and `panel_side`.
`event_loop` routes keys in the precedence documented in
[keybindings.md](keybindings.md): app-level accelerators (view switch, help,
panel focus/scroll, brain open/close/new, quit) → captive modal → brain panel
(forward bytes) → active main view (`handlers` for tasks, `search_view` for the
brain-directory picker). `draw` renders the active main view in the main
panel, the brain panel beside it (`panel_side`), and any modal over the top.
`search_view.rs` is the brain-directory view's handler (its picker nav, in-place
open, PDF/delete confirms, and its own `menu` palette). The remaining
submodules (`handlers`, `keymap`, `palette`, `modals`, `links`, `draw_*`,
`app_*`, `shell`) are the tasks view's, unchanged from the port.

### Startup (`run_tui`)
`run_tui()` opens the state DB, builds the brain-search picker
(`build_search`), constructs the `App`, then `open_or_focus_brain(None)` spawns
the initial `claude` PTY (resume-vs-fresh) and `focus_tasks()` returns focus to
the tasks main view so `j`/`k` work at once. It runs the startup daily-triage
check, then the event loop; on return it releases the session lock. The brain
panel is **closeable** (claude exit → `close_brain` drops the PTY and the main
view goes full-width); `open_or_focus_brain` (`Ctrl+M`) re-opens it. The
brain-directory view keeps its own `scope`/`rescope`/`search_refresh` for
bucket rescoping (`Ctrl+R` / palette search rows). Unlike the pre-merge shell
there is no `Exit` enum — the shell just returns from the event loop on quit
(the tasks view never handed a plan back), and `Ctrl+T`/`Ctrl+B` switch views
in-process rather than exiting.

### `pty_pane.rs`
`PtyPane` spawns a shell command under a pseudoterminal (`portable-pty`),
streams its bytes through a `vt100` parser, and exposes the screen for
rendering. Reader / writer / waiter threads; `send` / `resize` /
`scroll_*` / `is_alive`. A near-verbatim port of `tasks/src/pty_pane.rs`.

### `session.rs`
Pure launch planning: `Plan::{Resume,Fresh}` (chosen from the DB's resume
candidate + a fresh UUID), `build_claude_command` (`cd <root> && claude
--resume <id>` or `--session-id <id>`, using bare `claude` to control the
flag), and `env_for` (the `BRAIN_INSTANCE_ID` / `BRAIN_PID` /
`BRAIN_STATE_DB` env handed to the child for the SessionStart hook).

### `state.rs`
The SQLite state layer (`rusqlite`, WAL) at `~/.cache/brain/state.db`.
`brain_sessions` tracks every Claude session brain launched/adopted with a
`locked_pid` lock; `meta` stores the `panel_side` layout preference. The
resume model is **lock + recency** (`reap_dead_locks`, `pick_resume`,
`claim`, `register_fresh`, `release`). The `PanelSide` enum lives here since
it's the persisted value. Mirrors `tasks/src/state`. See
[data-model.md](data-model.md) and [integrations.md](integrations.md).

### `plan.rs`
The wire protocol. Each `cd` / `claude` / `open` / `edit` / `tasks`
helper writes one or two `key=value` lines to stdout. Each has a `*_to`
variant that writes to an arbitrary `io::Write` so the exact protocol
strings are a checked contract in tests.

### `lib.rs`
Re-exports the modules so integration tests in `tests/` can link against
them. The binary (`main.rs`) declares the same modules privately; with a
`lib.rs` present the source files compile into both a bin and a lib crate
(the same pattern `tasks` uses).

## Build / run loop

The `brain` zsh function rebuilds `target/release/brain` whenever
`Cargo.toml` or any `src/**/*.rs` is newer than the binary, then execs it.
The user never types `cargo run`. Manual rebuild:

```sh
( cd ~/src/jpsyx/brain && cargo build --release )
```

## Invariants the code depends on

- **`Bucket` declaration order is the display order** (Projects → Areas →
  Resources → Archive). The picker's `sort_by` and `build_display_rows`
  rely on the derived `Ord`.
- **The binary's stdout is *only* the plan.** Any other stdout would be
  parsed as directives or echoed by the wrapper. Diagnostics go to stderr;
  the TUI goes to `/dev/tty`.
- **Every `Choice` has exactly one palette row** (guarded by a test on
  `items(side, …)`) so the menu can't silently drop an action.
- **The brain panel is open at startup but closeable.** `tui` spawns the
  `claude` PTY at startup (resumed or fresh) and is two-panel; when claude
  exits the panel **closes** (search goes full-width) — it does not quit the
  shell. `open_or_focus_brain` ("Message brain" / `Ctrl-M`) re-opens it.
- **Exactly one Claude session per brain instance is locked at a time.**
  The SessionStart hook frees the instance's other sessions on every start
  (so `/new` leaves the prior conversation resumable); `release` clears the
  lock on exit; dead-PID locks are reaped on the next startup.

## Dependencies

The persistent shell pulls in four crates beyond the one-shot picker's set,
all mirroring the `tasks` sibling so the two projects share a stack:

- `portable-pty` + `vt100` + `tui-term` — spawn, parse, and render the
  embedded `claude` PTY.
- `rusqlite` (`bundled`) — the WAL state DB shared with the SessionStart
  hook; `bundled` avoids a system libsqlite dependency.
- `uuid` (`v4`) — per-shell brain-instance ids and fresh session ids.
