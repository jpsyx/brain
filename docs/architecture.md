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
  ├─ `brain --verbose …` → writes a timestamped `/tmp` log; non-TUI mirrors logs to stdout
  ├─ `brain config …`  → prints the config table / a value to stdout
  ├─ `brain tasks … --no-tui | complete | doctor` → plain output / mutate / check
  └─ everything else   → opens the persistent TUI on /dev/tty
```

The TUI renders to `/dev/tty`, so the binary's **stdout** is only what
plain CLI surfaces print (`config`, `env`, `version`, clap help/errors) plus
explicit `--verbose` log mirroring in non-TUI mode. Verbose TUI runs keep
stdout quiet and expose the log through the tasks command palette. The binary
opens files, cds its own PTY, launches `claude`, and reveals in Finder itself,
from inside the running shell. See [decisions.md](decisions.md) for *why* it is
a pure TUI binary, and [integrations.md](integrations.md) for the launch/handoff
detail.

## High-level data flow (inside the binary)

```
argv
 └─→ Cli::parse                          (cli.rs)
      ├─→ -v / --version / Cmd::Version ─→ print crate version and exit before any gates
      ├─→ --verbose ─→ logging::init      (timestamped `/tmp` log; stdout mirror unless TUI)
      ├─→ Cmd::Config ─→ config_command   (list/get/set; runs BEFORE the gate)
      ├─→ Cmd::Env ─→ env_command         (list/get/set over env.json; also BEFORE the gate)
      ├─→ Cmd::Sync ─→ sync_command       (sync/--push/--pull/setup/repair/status/conflicts; also BEFORE the gate)
      ├─→ Cmd::Check ─→ sync::check::run  (read-only dry-run push/pull report; also BEFORE the gate)
      ├─→ Cmd::Personalize ─→ personalize_command (show/get/set/edit / onboarding; also BEFORE the gate)
      ├─→ Cmd::Server ─→ server_command   (start/status/kill/run — the background HTTP daemon; also BEFORE the gate)
      └─→ settings::ensure_markdown_to_pdf (prereq gate: config path, else discover; red ❌ + exit if unresolved)
           ├─ no subcommand ─────────→ tasks_launch(default view) → tui::run_tui (MERGED SHELL, tasks view)
           └─ Cmd::Tasks(rest)       ─→ TasksCli::parse_from(rest) → tasks_launch:
                                          complete → complete::run (native CSV completion)
                                          doctor   → doctor::run_doctor
                                          --no-tui → plain::print_plain
                                          else     → tui::run_tui (MERGED SHELL)

tui::run_tui(view, cli, …)                  (the persistent shell)
 ├─→ paths::brain_root()                     (env `root` → legacy pointer → else $HOME/brain)
 ├─→ build_search(brain_root)                (entry::collect over all buckets → picker::App)
 └─→ App event loop (tasks view + search view + agent PTY)
       ├─ state::Db: reap dead locks, pick_resume / claim or register_fresh
       ├─ session::build_llm_command(root, agent_kind, command, …) + env_for
       │    → PtyPane spawns configured Claude (default) or Codex (`--codex` / `-cx`)
       ├─ Ctrl+L/H cycle views, Ctrl+T/B jump; Alt+H/L switch panel focus
       ├─ Ctrl+P opens a command palette (tasks: tui::palette; search: menu::MenuApp; verbose TUI adds "Show logs")
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
utility (`complete` → native CSV completion; `doctor` → `run_doctor`; `--no-tui`
→ `plain::print_plain`) or opens the merged shell via `tui::run_tui`. There is
no plan and no `Exit` mapping: the shell just returns when the user quits.

### `cli.rs`
The clap derive surface. `Cli` owns the global flags (`-v`/`--version` and
`--verbose`) plus one optional `Cmd`. Bare `brain` (no `Cmd`) is equivalent to
`brain tasks` — the tasks view is the startup default.

### `logging.rs`
Optional per-run verbose logging. `logging::init` creates a timestamped
`/tmp/<rfc3339-nanos>.log` file only when `--verbose` is present, mirrors log
lines to stdout for non-TUI commands, and prints the final log path at process
exit. Before the persistent shell takes over `/dev/tty`, `main.rs` disables the
stdout mirror; the TUI keeps the log path in `App` and offers **Show logs** in
the tasks command palette. Command handlers and thin IO shells call
`logging::log` at phase boundaries: dispatch, config/env/personalize actions,
task CSV loads and writes, sync/rclone work, server lifecycle probes, doctor
checks, and skill installation.

### `paths.rs`
Brain-root resolution. `brain_root()` reads the configured root and creates the
resolved directory (including missing parents) when a command needs it;
`brain_root_path()` is the side-effect-free variant used to derive the config
dir. `root` is deliberately *not* a config variable — it can't live inside
the brain root it resolves. The IO-free pieces (`parse_brain_root_file`,
`expand_tilde_with_home`) are split out so they're unit-testable without a
real `$HOME` or pointer file. See [config.md](config.md).

### `settings/`
The persistent config store (`<brain-root>/.config/config.json`) and the
`brain config` command. Owns the raw JSON read/modify/write, the declared-
variable schema, get/set/list (with the aligned, colored `config list` table),
and the `markdown-to-pdf` prerequisite: auto-discovery (PATH → conventional bin
dirs → login-shell resolution of a function wrapper), validation, and the
fail-fast red-`❌` startup gate. Pure decision helpers (schema resolution, table
layout, message wording, shell-output parsing) are unit-tested; the IO shells
are thin. Split into `store` (JSON IO), `schema` (`VARS`/`Resolved`), `vars`
(get/set/resolve), `render` (the `config list` table), and `markdown_pdf` (the
prerequisite). See [config.md](config.md).

### `personalization/`
The personalization store — content *about you* at
`<brain-root>/.config/personalization.json` (beside the config store in
`settings::config_dir()`; it is just another brain config, inside the brain root
so it travels with the brain). Split into `model` (the `Personalization` schema +
parse), `store` (path resolution in the brain config dir + load/save), `tags`
(the `TagStyle`/`TagStyles` model, the
generic defaults `mit`/`personal`/`work`, and pure label resolution with
raw-name fallback), `runtime` (a process-cached copy of the resolved tag styles
so the renderer resolves labels without threading state), `command` (the
`brain personalize` show/get/set/edit logic — pure helpers + thin IO), and
`onboarding` (the skippable first-run prompt). The task renderer's
`type_label` delegates here, so the public binary carries no personal taxonomy.
See [config.md](config.md) and [data-model.md](data-model.md).

### `skills/`
The brain skill pipeline (sub-project B): render the bundled skills and install
them into the shared agent registry (`~/.agents/skills`), fanning out to each
frontend (Claude, **Codex**, OpenCode, Cursor). Split into `model` (the shared `Skill`/`SkillFile` type), `embed` (the
`include_dir!`-embedded `skills/` dir → bundled `Skill`s), `plugin` (whole user
skills discovered from `<root>/.config/plugins/<name>/`), `extension` (parse a
`<root>/.config/extensions/<skill>.md` into named `[hook]` sections + catch-all,
and `apply` it to a base `SKILL.md` at `<!-- brain:ext hook -->` markers,
producing a *new built copy* only — never the repo/plugin source; unmatched
content lands in a trailing "Personal extensions" section), `render` (base skill
→ installable files, injecting the extension into `SKILL.md`), `layout` (the
built dir + registry + frontend dirs, and the pure `link_ops` target
computation), `install` (collect bundled + plugins, write built + create the
two-hop symlinks; thin FS shell over `link_ops`), and `command`
(`brain skills sync [--root <sandbox>]`; `format_sync_plan` prints the built
dir, registry, frontend count, and extension/plugin sources before the FS shell
runs). `resync_skills()` (the A seam) runs the pipeline, gated by
`skills_auto_sync` (**default `true`** since the B4 cutover) so a mutation
re-renders the live registry; set the flag `false` to manage skills only via
explicit `brain skills sync`. jpsyx delegates to `brain skills sync` and never
prunes brain-owned links (they resolve into brain's built dir, outside jpsyx's
sources). See the B spec under `docs/superpowers/specs/`.

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
and `day_rollover_hour`; `linear_base_url()` interpolates the workspace slug
into the full issue-URL prefix. Missing file/fields fall back to defaults, and
keys read elsewhere (e.g. `agenda_dir`, `skills_auto_sync`, or brain-env values
in the separate `env.json`) are ignored here.

### `sync/`
`brain sync`: manual, bidirectional cross-machine sync of the brain root to a
private Backblaze B2 bucket via `rclone bisync`, dispatched in `main.rs`
**before** the `markdown-to-pdf` prerequisite gate (like `config`/`env`/
`personalize`/`skills`). The data flow per run is **build → run → post-pass →
verify → journal**: `config` (`SyncConfig`, parsed from the brain-env `sync`
block) feeds `remote::build_remote` (the B2 remote as `RCLONE_CONFIG_*` env
vars, never on argv) and `args::bisync_args` (the full `rclone bisync` argv:
conflict resolution bias for the direction, keep-both flags, `--max-delete`,
default excludes, `--check-access --check-filename RCLONE_TEST`, plus
`--stats 10s --stats-one-line` for live progress and `--resilient --recover`
for resumability); `check_access.rs` creates/repairs the root-level marker on
local + remote before resync runs; `run::run_rclone` checks for the external
`rclone` executable before spawning it, then streams its stderr live to the terminal while
capturing it, and parses the capture into transferred/deleted/error counts
and an abort reason; if that abort is an incomplete baseline
(`AbortKind::PriorListingMissing`) and this run wasn't already a resync,
`command::sync_once` auto-resumes with one internal `Direction::Resync` retry
before continuing (journalled as such) — see [decisions.md](decisions.md);
`conflicts::rename_markers` (the post-pass) renames any rclone-left conflict
marker (`<original>.__brainconflict__<N>`) to the friendly
`name (conflict <host> <date>).ext`; `verify::classify` turns the parsed
outcome + the count of copies renamed + any leftover markers into `Clean` /
`NeedsAttention` / `Aborted` (a run that created any conflict copy is
`NeedsAttention` even after the markers are renamed away); and
`journal::Journal` records the run into the
SQLite journal at `~/.cache/brain/sync/journal.db` (table `sync_runs`,
machine-local, never synced). `command::sync_once` is the thin orchestrator
that runs this whole pipeline; `command::print_status`/`print_conflicts` back
`brain sync status`/`brain sync conflicts`. `setup.rs` is `brain sync setup`'s
interactive flow (collect bucket + credentials, verify/create the bucket via
`rclone lsd`/`mkdir`, write the `sync` block into brain env, bootstrap the
check-access markers through `sync_once(Direction::Resync)`, then run one
baseline `sync_once` with `Direction::Resync`) — `brain sync repair` reruns just
that resync on an already configured machine, mainly as the recovery path for
rclone's own "prior listings missing" guard. See
[integrations.md](integrations.md) for the rclone handoff detail and
[data-model.md](data-model.md) for the `sync` config fields and the journal
schema. `rclone` is an external dependency (not a Cargo crate): a soft
prerequisite, checked only when `brain sync` actually runs, never a startup
gate (`brain tasks doctor` reports its presence/version informationally).

`check.rs` backs `brain check`, a **read-only** sibling of `sync_once`: it
builds the same `Direction::Both` argv via `args::bisync_args` but appends
`--dry-run`, runs it through `run::run_rclone_capture` (a quiet, non-streaming
counterpart to `run::run_rclone` — no live terminal output, just `(exit_ok,
combined_output)`), then classifies the captured detection-phase lines with
`progress::classify_change`/`Side` (the same parser `progress.rs` already
exposed for a future live file-list). It then runs the CSV lane's read-only
counterpart: `check::collect_csv_pending_with_fetch` reads the cached
`csv_sync::baseline_path` text and the local task/habit CSVs, fetches each
remote CSV through `csv_sync::remote_csv_arg` + rclone `copyto`, and compares
both sides to the baseline with `csv_merge::parse`-backed row diffs. The pure
`check::format_report` receives both the file path lists and the CSV row
counts for the themed summary. The command prints default progress before the
rclone dry-run and before the CSV baseline pass. No journal entry, no conflict post-pass, no
baseline mutation: it never calls `rclone bisync` without `--dry-run`, and
its CSV pass never writes local files, remotes, or baselines.

**The C4 auto-sync trigger layer** (`lock.rs`/`watch.rs`/`trigger.rs` plus the
idle-pull timer, wired
into the shell lifecycle) makes sync automatic while keeping the pure/impure
split. The **pure** cores carry the decisions and the tests; the thin shells do
the IO/threads/`Command`:

- `lock.rs` — the machine-wide advisory sync lock at
  `~/.cache/brain/sync/sync.lock` (a PID file beside the journal). Pure
  `is_stale(owner_alive, age, cap)` decides reap-ability (dead owner or
  heartbeat mtime past the cap); `try_acquire(path)` is the atomic
  (`create_new`/O_EXCL) thin IO shell returning `Option<Guard>` (`None` when a
  live, fresh sync holds it), and `Guard` owns a heartbeat thread that refreshes
  the lockfile mtime until drop. Drop stops the heartbeat and removes the file
  only if it still holds our PID. It wraps **all** sync entry points, including
  the manual `run_sync` in `main.rs`, closing a pre-existing concurrent-`brain
  sync` race.
- `watch.rs` — the pure `Debouncer` (a clock-injected quiescence state machine:
  `on_event`/`time_until_fire`/`poll`) and the pure `is_watch_relevant(path)`
  exclude predicate, plus the thin `notify` shell `spawn_watcher_with` (owns the
  `RecommendedWatcher`, the mpsc event channel, and the debounce loop) and
  `spawn_watcher` (the real auto-sync watcher). `WatcherHandle` stops the thread
  on drop (drop the `Watcher` → the channel disconnects → the loop exits; no
  join, so teardown never blocks). On fire it only *spawns a detached background
  sync*; the sync's own writes re-arm the debouncer, but a spawn that lands
  while a sync holds the lock coalesces (exits silently), so there is no loop.
- `trigger.rs` — the single shell-facing entry point: `spawn_detached_sync(dir)`
  spawns the current exe as `brain sync [--pull|--push] --if-idle`, fully
  detached (`process_group(0)` + null stdio). **Every** automatic trigger
  (start, watcher, idle, exit) goes through it, for two reasons: a sync in a
  separate process can never write over the TUI, and a detached child in its own
  process group outlives the shell / terminal close. `--if-idle` makes a
  redundant trigger coalesce (exit silently) rather than follow. There is no
  in-process sync path anymore (the old `run_locked_sync`/`sync_in_background`
  are gone).
- `current.rs` — the in-flight sync's shared state, so a detached background
  sync stays observable. `Reporter` is the single output sink of a run: each
  line is appended to `~/.cache/brain/sync/current.log` and echoed to the
  process's own stderr (the terminal for a foreground run, `/dev/null` for a
  detached one). `begin` writes the `current.json` record (pid + direction +
  start); `Drop` removes it. Pure `running(state, owner_alive)` +
  `is_running()` gate on the record existing *and* its owner PID being alive, so
  a hard-killed sync's stale record never reads as live.
- `follow.rs` — when a user-run `brain sync` finds the lock held, it attaches
  instead of erroring: `follow_until_done` tails `current.log` (pure
  `appended(content, offset)` splits off each new tail, handling a truncated
  log) to the terminal until `is_running()` goes false, then prints the final
  journal outcome. Ctrl-C stops only the follower.
- `idle.rs` — the shell-lifetime idle-pull timer. `spawn_idle_puller_with`
  owns the stop channel and injected callback for tests; `spawn_idle_puller`
  spawns a detached `brain sync --pull --if-idle` every `sync.idle_pull_secs`
  seconds when that opt-in interval is configured.
- `config.rs` carries `debounce_ms` (default 3000), `debounce() -> Duration`,
  and the opt-in `idle_pull_secs` / `idle_pull_interval()` pair;
  `command::format_triggers` renders both intervals in `brain sync status`.

**The `run_tui` lifecycle seam** (`src/tui/event_loop/setup.rs`) is the one wire
point: after the startup work and before the event loop it calls
`trigger::spawn_detached_sync(Both)` (when `on_start`) and holds a
`watch::spawn_watcher` handle (when `watch_effective()`) plus an
`idle::spawn_idle_puller` handle (when `idle_pull_secs > 0`); after the loop
returns it calls `trigger::spawn_detached_sync(Both)` (when `on_exit`) and drops
the watcher/timer handles. All gated, all best-effort: an unconfigured brain
gets no watcher thread, no timer, and no syncs. This layer adds no keybinding,
palette row, or menu row.

**The C5 conflict enumerator + resolver** builds on `conflicts.rs` to give
agents (not just humans) a way to close out a keep-both conflict. Still pure
where it can be:

- `conflicts.rs` gains `parse_conflict_name` (the strict inverse of
  `conflict_name`: friendly name → `ParsedConflict { original, host, date }`,
  or `None` on anything that isn't the exact grammar), `group_conflicts`
  (folds flat `list_conflicts` output into `ConflictGroup`/`ParsedCopy`,
  sorted for deterministic output), and `copies_for_original` (the copies
  belonging to one canonical original, used by `resolve`).
- `command/mod.rs` gains `conflict_display_paths`, which renders the human
  line-list from the same strict groups, and `conflicts_json`, a pure builder
  (fs metadata and existence checks injected as closures) that renders
  `&[ConflictGroup]` into the `serde_json::Value` array `brain sync conflicts
  --json` prints — see [data-model.md](data-model.md) for the exact shape.
- `command/resolve.rs` backs `brain sync resolve <original> [...]`: the pure
  `resolve_decision` classifies each original as `Delete(copies)` /
  `CanonicalMissing` / `NoCopies`, and the thin shell around it deletes the
  copies (never the canonical file) and never touches rclone or the journal.
  Bare `brain sync resolve` (no args) drops into an interactive picker over
  the open conflict groups.

Together these back the `/second-brain resolve-conflicts` skill: it reads
`conflicts --json`, merges each group into its canonical file, deletes the
copies via `resolve`, then runs one ordinary `brain sync`. See
[integrations.md](integrations.md) for that contract in full.

### `tasks/`
Everything specific to the **tasks main view**, ported from the old `tasks`
crate under one namespace: `task` (CSV model + load), `view` (sub-views +
`build_view`), `selector` (date parsing), `render` (task-card lines, chrome,
markdown), `shortcuts` (the help/footer catalogue), `complete` (native
task/habit completion), `doctor` (health check), `plain` (`--no-tui` printer),
and `cli` (the tasks clap args, nested under `brain tasks`). Reuses the
crate-level `session` / `state` / `pty_pane` shared with the brain-search view.

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
check, then wires the auto-sync triggers (a detached `on_start` background sync,
when `watch_effective()`, a held `watch::spawn_watcher` handle, and when
configured, a held idle-pull timer), runs the event loop, and on return fires
the detached `on_exit` sync, drops the watcher/timer handles, and releases the
session lock. The brain
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
Pure launch planning: `AgentKind::{Claude,Codex}`, `Plan::{Resume,Fresh}`
(chosen from the DB's resume candidate + a fresh UUID for Claude, fresh-only
for Codex today), `build_llm_command` (`cd <root> && <claude_cmd> --resume
<id>` / `--session-id <id>` for Claude; `cd <root> && <codex_cmd> resume <id>`
for a known Codex resume id; no Claude flags for fresh Codex), and `env_for`
(the `BRAIN_INSTANCE_ID` / `BRAIN_PID` / `BRAIN_STATE_DB` env handed to Claude
for the SessionStart hook). `claude_cmd` and `codex_cmd` are machine-local brain
env values. Both configured commands are spliced in verbatim so they may carry
their own flags, and brain never depends on a shell alias.

### `state.rs`
The SQLite state layer (`rusqlite`, WAL) at `~/.cache/brain/state.db`.
`brain_sessions` tracks every Claude session brain launched/adopted with a
`locked_pid` lock; `meta` stores the `panel_side` layout preference. The
resume model is **lock + recency** (`reap_dead_locks`, `pick_resume`,
`claim`, `register_fresh`, `release`). The `PanelSide` enum lives here since
it's the persisted value. Mirrors `tasks/src/state`. See
[data-model.md](data-model.md) and [integrations.md](integrations.md).

### `server/`
The **brain server**: a small, synchronous, localhost HTTP daemon (`tiny_http`),
one shared instance per machine across all `brain` invocations and tabs. Its
`{pid, port}` record lives at `~/.cache/brain/server.json`.
- `server/router.rs` — pure `route(method, path) -> Route` (`HabitsPage` for
  `GET /habits`, `HabitsDone` for `POST /habits/done`, `WebhookCapture` for
  `POST /webhooks/capture`, `NotFound` for everything else including the bare
  root `/`); query strings are stripped before matching.
- `server/lifecycle.rs` — the daemon record + management: pure `is_live` and
  `choose_port` decisions, thin IO probes (`read_state`/`write_state`/
  `remove_state`, `pid_alive` via `kill -0`, `port_reachable` via a timed TCP
  connect), `running()` (reap-if-stale), `ensure_running()` (reuse-or-spawn),
  `format_ensure_plan` (the progress line printed before CLI waits on daemon
  reuse/spawn), and the `start`/`status`/`kill` CLI actions.
- `server/mod.rs` — `run(port)`, the blocking accept loop the detached daemon
  runs: binds `127.0.0.1:port` (`0` = OS-assigned), writes the actual bound
  port to the record, then dispatches each request through the router to a
  handler in `server/routes/`.
- `server/routes/` — the route registry (`routes/mod.rs` is one `pub mod` line
  per endpoint). `routes/habits/` is the `/habits` route in MVC form:
  `model.rs` (pure `classify` filter+sort of today's habits over a `Habit`
  struct, plus the thin `load` reader of `<root>/tasks/habits.csv`), `view.rs`
  (pure HTML rendering into the `web/habits/` shell, with `style.css`/`app.js`
  inlined via `include_str!`), and `mod.rs` (the thin controller: `page` =
  load→classify→render, `done` = parse body → reuse native `tasks::complete`
  completion → `DoneOutcome`). `routes/webhooks/` owns the generic
  `/webhooks/capture` endpoint: pure-ish filename/response decisions plus the
  thin write to `<root>/scratch/webhooks/<timestamp>-<seq>.<json|txt>`.

The daemon is spawned detached without `unsafe`: `CommandExt::process_group(0)`
plus null stdio on the current exe (`brain server run --port <p>`).
`tasks_launch` best-effort calls `ensure_running()` so the server comes up with
the shell. The frontend assets live at the repo root under `web/habits/`
(`index.html` shell + `style.css` + `app.js`), embedded into the binary at
compile time.

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
- `include_dir` — embeds the repo's `skills/` dir (SKILL.md + scripts) into the
  binary so a public cloner needs no repo checkout; `brain skills sync` writes
  them out. Multi-file skill assets rule out `include_str!`.
- `tiny_http` — the **brain server** (`src/server/`): a small, synchronous,
  blocking-IO HTTP server for a local-only service (the habits view today,
  webhook POST endpoints later). Chosen over axum/actix specifically to avoid
  pulling a Tokio async runtime into an otherwise synchronous CLI; the brain
  server is a tiny localhost daemon, so an async stack would be pure overhead.
- `notify` (8.x) — OS-native filesystem events (FSEvents on macOS, inotify on
  Linux) for the **C4 auto-sync watcher** (`src/sync/watch.rs`). It is the only
  correct, cross-platform, OS-native FS-event crate; the alternative is a
  polling loop we explicitly rejected as wasteful (it would burn CPU walking the
  tree on a timer, where `notify` blocks on a channel and costs nothing when
  idle). We use the raw `RecommendedWatcher` and do the debouncing in our own
  tested `watch::Debouncer`, so we depend on neither `notify-debouncer-full` nor
  `notify-debouncer-mini` and the decision logic stays pure and ours.

`brain sync` also depends on **`rclone`**, but as an external command it
shells out to (`src/sync/run.rs`), not a Cargo crate: brain builds the argv
and an env-var-only remote config and lets the user's own `rclone` install do
the transfer. It's a soft prerequisite (checked only when `brain sync` runs;
see [integrations.md](integrations.md)), unlike the hard `markdown-to-pdf`
gate.
