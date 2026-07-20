# Architecture

`brain` is a small Rust CLI that browses `~/brain` (a PARA-organized second
brain: projects / areas / resources) and acts as the single terminal
entry point for the user's knowledge and task workflows.

As of the tasks↔brain merge, `brain` is the single CLI for both the second
brain and the task system; the standalone `tasks` binary is gone. It has two
surfaces:

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
- **The tasks utilities** (`brain tasks {complete|doctor|search|--no-tui …}`)
  and **`brain config {list|get|set}`** are short-lived: they mutate the CSVs,
  run a health-check, print plain output, or read/write config, then exit.

There are **no** shell-mutating one-shot commands: no `cd`, `msg`, or
per-bucket search subcommand, and no freeform note search. Everything the
user does happens *inside* the persistent shell, which performs its own
file-open, Finder-reveal, PDF, trash, and `claude`-launch actions by
spawning processes. So the binary needs no parent-shell cooperation, no
wrapper, and no plan protocol.

## One binary, run directly

```
user types `brain …`
  └─→ run.sh
       ├─ rebuilds the binary if any src/*.rs (or Cargo.toml) is newer
       │    (cargo build --release; build chatter → stderr)
       └─ exec target/release/brain "$@"   (forwards every argument)

the binary:
  ├─ `brain config …`  → prints the config table / a value to stdout
  ├─ `brain tasks … --no-tui | complete | doctor` → plain output / mutate / check
  └─ everything else   → opens the persistent TUI on /dev/tty
```

The TUI renders to `/dev/tty`, so the binary's **stdout** is only what
`brain config` prints plus clap's help/errors. The binary opens files, cds
its own PTY, launches `claude`, and reveals in Finder itself, from inside the
running shell. See [decisions.md](decisions.md) for *why* it is a pure TUI
binary, and [integrations.md](integrations.md) for the launch/handoff detail.

## High-level data flow (inside the binary)

```
argv
 └─→ Cli::parse                          (cli.rs)
      ├─→ Cmd::Config ─→ config_command   (list/get/set; runs BEFORE the gate)
      └─→ settings::ensure_markdown_to_pdf (prereq gate: config path, else discover; red ❌ + exit if unresolved)
           ├─ no subcommand ─────────→ tasks_launch(default view) → tui::run_tui (MERGED SHELL, tasks view)
           └─ Cmd::Tasks(rest)       ─→ TasksCli::parse_from(rest) → tasks_launch:
                                          complete → complete::run (exec mark_done.py)
                                          doctor   → doctor::run_doctor
                                          --no-tui → plain::print_plain
                                          else     → tui::run_tui (MERGED SHELL)

tui::run_tui(view, cli, …)                  (the persistent shell)
 ├─→ paths::brain_root()                     (config `root` → else $HOME/brain)
 ├─→ build_search(brain_root)                (entry::collect over all buckets → picker::App)
 └─→ App event loop (tasks view + search view + claude PTY)
       ├─ state::Db: reap dead locks, pick_resume / claim or register_fresh
       ├─ session::build_claude_command(root, config.claude_command(), …) + env_for
       │    → PtyPane spawns the configured `claude …`
       ├─ Ctrl+L/H cycle views, Ctrl+T/B jump; Alt+H/L switch panel focus
       ├─ Ctrl+P opens a command palette (tasks: tui::palette; search: menu::MenuApp)
       ├─ Enter on a file opens it in place (open_target spawners) — shell stays up
       └─ quit → the loop just returns (no plan, no wrapper handoff)
```

Both main views share the pure picker logic (`picker::App` matching /
navigation, rendered via `picker::draw_into`) and the `menu` palette in the
search view. The search panel lives in a bordered half of the shell alongside
the live brain panel; opening a file or rescoping a bucket happens in place
and the shell stays up.

## Modules

### `main.rs`
Owns argv → `Cli` and the top-level `match` over `Cmd`. `brain config …` is
dispatched first (before the `markdown-to-pdf` gate). Bare `brain` and
`brain tasks …` both flow into `tasks_launch`, which either runs a tasks
utility (`complete` → exec `mark_done.py`; `doctor` → `run_doctor`; `--no-tui`
→ `plain::print_plain`) or opens the merged shell via `tui::run_tui`. There is
no plan and no `Exit` mapping: the shell just returns when the user quits.

### `cli.rs`
The clap derive surface. `Cli` is a single optional `Cmd`. `Cmd` has just two
variants: `Tasks(TasksArgs)` (all args after `tasks` forwarded verbatim to the
tasks CLI parser) and `Config(ConfigArgs)` (with a `list`/`get`/`set`
subcommand). Bare `brain` (no `Cmd`) is equivalent to `brain tasks` — the
tasks view is the startup default.

### `paths.rs`
Brain-root resolution. `brain_root()` reads the config store's `root`
(tilde-expanded) or falls back to `$HOME/brain`, erroring if the result
isn't a directory. The IO-free pieces (`parse_config_root`,
`expand_tilde_with_home`) are split out so they're unit-testable without a
real `$HOME` or config file. See [config.md](config.md).

### `settings/`
The persistent config store (`~/.config/brain/config.json`) and the
`brain config` command. Owns the raw JSON read/modify/write, the declared-
variable schema, get/set/list (with the aligned, colored `config list` table),
and the `markdown-to-pdf` prerequisite: auto-discovery (PATH → conventional bin
dirs → login-shell resolution of a function wrapper), validation, and the
fail-fast red-`❌` startup gate. Pure decision helpers (schema resolution, table
layout, message wording, shell-output parsing) are unit-tested; the IO shells
are thin. Split into `store` (JSON IO), `schema` (`VARS`/`Resolved`), `vars`
(get/set/resolve), `render` (the `config list` table), and `markdown_pdf` (the
prerequisite). See [config.md](config.md).

### `entry.rs`
`Bucket` (Projects / Areas / Resources / Archive; declaration order =
display order, Archive last) and `Entry` (absolute `path`, `~/brain/...`
`display`, `bucket`).
`collect()` walks each root with `walkdir`, skips hidden files
(`.`-prefixed) and the root itself, and tags every entry with its bucket.
Missing roots are silently skipped.

### `picker/`
The ratatui fuzzy picker. The `App` type lives in `picker/mod.rs` (so every
submodule reaches its private fields); the impl is split into `haystack`
(match preprocessing), `filter` (constructors + `refilter` + grouping), `nav`
(query edits + cursor + scroll), `selection` (highlighted-entry accessors +
palette/confirm openers), and `view` (`draw_into`). `App` **owns** its entries (so the persistent
shell can `set_entries` to rescope a bucket in place), precomputed
`HaystackBuf`s, the query, the current matches, and the interleaved
header/match `display_rows`. `refilter()` runs nucleo substring matching,
sorts matches by bucket then score then walk order, and rebuilds the
section-grouped rows. Navigation (`move_up`/`down`, `page_*`,
`ensure_visible`) keeps the cursor and its section header on screen.
Rendering is delegated to `render.rs` and exposed as `draw_into(f, app,
area)` so `tui`'s embedded search panel paints it. `App` also holds an
optional `palette: Option<menu::MenuApp>` overlay and an optional
`confirm: Option<confirm::Confirm>` overlay (routed before the palette) that
serves both the "Create PDF" and "Delete" confirmations. The `App` is driven
key-by-key by the search view (`tui/search_view.rs`): `Enter` opens the
selection in place (a directory falls back to a Finder reveal), `Ctrl-Enter`
reveals, `Ctrl-G` confirms a markdown→PDF conversion, `Ctrl-D` confirms a
trash, and a confirmed palette row runs its action. Every action happens in
place; the shell never tears down on a selection. On `Accept`, Delete trashes
the path and `drop_path`s the entry (`reload_entries` keeps the query), and
the picker stays open.

### `menu/`
Split into `labels` (contextual-row elision), `model` (`Choice`/`Targets`/the
row list/`shortcut_for`), `filter` (the substring matcher), `app` (`MenuApp` +
`handle_key`), and `view` (`draw_modal`).
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
overlay, not to exit. In the persistent shell `Msg` opens/focuses the brain
panel and `ToggleLayout` swaps which side it sits on.

### `confirm.rs`
The shared yes/no confirmation modal. Like `menu`, it has **no screen of its
own**: the picker holds a `Confirm { path, kind, yes }` in its state, the host
drives its pure `handle_key` (returns `Continue`/`Cancel`/`Accept`), and paints
it with `draw_modal` as a centered overlay. `ConfirmKind` selects the flavor:
**Pdf** (green, defaults to Yes; opened by `Ctrl-G` on a `.md` file) and
**Delete** (red, defaults to **No** because it's destructive; opened by
`Ctrl-D` on any entry). The pure `accent`/`title`/`question` helpers key off
`kind`; on `Accept` the host converts (Pdf) or trashes (Delete) in place. The
key handling, the kind-keyed chrome, and the button styling are unit-tested.

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
Typed view of the runtime knobs, deserialized from the shared config store
(see `settings/`). Fields: `daily_triage_name_pattern`, `linear_workspace`,
`day_rollover_hour`, and `claude_cmd`; `linear_base_url()` interpolates the
workspace slug into the full issue-URL prefix, and `claude_command()` returns
the configured brain-panel launch command (or the default `claude
--dangerously-skip-permissions` when blank). Missing file/fields fall back to
defaults, and unknown keys (`root`, `markdown_to_pdf_path`, read elsewhere)
are ignored.

### `tasks/`
Everything specific to the **tasks main view**, ported from the old `tasks`
crate under one namespace: `task` (CSV model + load), `view` (sub-views +
`build_view`), `selector` (date parsing), `render` (task-card lines, chrome,
markdown), `shortcuts` (the help/footer catalogue), `complete` (exec
`mark_done.py`), `doctor` (health check), `plain` (`--no-tui` printer), and
`cli` (the tasks clap args, nested under `brain tasks`). Reuses the crate-level
`session` / `state` / `pty_pane` shared with the brain-search view.

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

The larger submodules are directories split by concern: `handlers/`
(`overlay`/`tasks_view`/`input`), `event_loop/` (`setup`/`modal_route`/`run`),
`draw/` (`tasks_panel`/`brain_panel`/`layout`, with the `draw` entry in
`draw/mod.rs`), `palette/` (`command`/`state`), `app_state/`
(`construct`/`nav`/`view`/`selection_query`), `app_actions/`
(`commands`/`triage`), and `tests/` (split by area). The overlay-modal state
structs (`PaletteState`, `ConfirmState`, `BrainInputState`, `HelpState`,
`LinkPickerState`, and the confirm enums) live in `modal_state.rs` with
`pub(super)` fields; `mod.rs` keeps only the `App` shell type, `Panel`,
`filter_tasks`, and the module wiring.

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
candidate + a fresh UUID), `build_claude_command` (`cd <root> && <claude_cmd>
--resume <id>` or `--session-id <id>`), and `env_for` (the
`BRAIN_INSTANCE_ID` / `BRAIN_PID` / `BRAIN_STATE_DB` env handed to the child
for the SessionStart hook). `claude_cmd` is the user-configurable launch
command (`config::Config::claude_command`, default `claude
--dangerously-skip-permissions`) spliced in verbatim so it may carry its own
flags; brain always appends the `--resume` / `--session-id` flag it controls,
so it never depends on a shell alias.

### `state.rs`
The SQLite state layer (`rusqlite`, WAL) at `~/.cache/brain/state.db`.
`brain_sessions` tracks every Claude session brain launched/adopted with a
`locked_pid` lock; `meta` stores the `panel_side` layout preference. The
resume model is **lock + recency** (`reap_dead_locks`, `pick_resume`,
`claim`, `register_fresh`, `release`). The `PanelSide` enum lives here since
it's the persisted value. Mirrors `tasks/src/state`. See
[data-model.md](data-model.md) and [integrations.md](integrations.md).

### `lib.rs`
Re-exports the modules so integration tests in `tests/` can link against
them. The binary (`main.rs`) declares the same modules privately; with a
`lib.rs` present the source files compile into both a bin and a lib crate
(the same pattern `tasks` uses).

## Build / run loop

`run.sh` rebuilds `target/release/brain` whenever `Cargo.toml` or any
`src/**/*.rs` is newer than the binary, then `exec`s it (build chatter goes
to stderr so stdout stays clean). The user never types `cargo run`. Manual
rebuild:

```sh
( cd path/to/brain && cargo build --release )
```

## Invariants the code depends on

- **`Bucket` declaration order is the display order** (Projects → Areas →
  Resources → Archive). The picker's `sort_by` and `build_display_rows`
  rely on the derived `Ord`.
- **The binary's stdout is *only* `brain config` output** (plus clap
  help/errors). No other path prints to stdout. Diagnostics go to stderr; the
  TUI goes to `/dev/tty`.
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

Beyond the picker's core (clap, ratatui, crossterm, nucleo, walkdir,
anyhow), the persistent shell pulls in four crates, all mirroring the `tasks`
sibling so the two projects share a stack:

- `portable-pty` + `vt100` + `tui-term` — spawn, parse, and render the
  embedded `claude` PTY.
- `rusqlite` (`bundled`) — the WAL state DB shared with the SessionStart
  hook; `bundled` avoids a system libsqlite dependency.
- `uuid` (`v4`) — per-shell brain-instance ids and fresh session ids.
