# Architecture

`brain` is a small Rust CLI that browses `~/brain` (a PARA-organized second
brain: projects / areas / resources) and acts as the single terminal
entry point for the user's knowledge and task workflows.

As of the tasks↔brain merge, `brain` is the single CLI for both the second
brain and the task system; the standalone `tasks` binary is gone. It has two
surfaces:

- **Bare `brain`** (and `brain tasks …`) opens a **persistent shell**
  (`tui/`) with **three main views** — the **tasks view** (task management,
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
  ├─ every run → writes a timestamped `/tmp` log; `--verbose` mirrors logs to stdout
  ├─ `brain config …`  → prints the config table / a value to stdout
  ├─ `brain tasks … --no-tui | complete | doctor` → plain output / mutate / check
  └─ everything else   → opens the persistent TUI on /dev/tty
```

The TUI renders to `/dev/tty`, so the binary's **stdout** is only what
plain CLI surfaces print (`config`, `env`, `version`, clap help/errors) plus
explicit `--verbose` log mirroring in non-TUI mode. TUI runs keep
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
      ├─→ logging::init                  (timestamped `/tmp` log; stdout mirror with `--verbose`)
      ├─→ Cmd::Config ─→ config_command   (list/get/set; runs BEFORE the gate)
      ├─→ Cmd::Env ─→ env_command         (list/get/set over env.json; also BEFORE the gate)
      ├─→ Cmd::Sync ─→ sync_command       (sync/--push/--pull/setup/repair/status/conflicts; also BEFORE the gate)
      ├─→ Cmd::Check ─→ sync::check::run  (read-only dry-run push/pull report; also BEFORE the gate)
      ├─→ Cmd::Reindex ─→ reindex::run    (rebuild derived lookup CSVs + task/habit rules; also BEFORE the gate)
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
       ├─ Ctrl+P opens a command palette (tasks: tui::palette; search: menu::MenuApp; status and log actions open the logs view)
       ├─ Enter on a file opens it in place (open_target spawners) — shell stays up
       └─ quit → the loop just returns (no plan, no wrapper handoff)
```

The task and brain-search views share the pure picker logic (`picker::App` matching /
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
Per-run logging. `logging::init` always creates a timestamped
`/tmp/<rfc3339-nanos>.log` file, and `--verbose` mirrors log
lines to stdout for non-TUI commands, and prints the final log path at process
exit. Before the persistent shell takes over `/dev/tty`, `main.rs` disables the
stdout mirror; the TUI keeps the log path in `App` and offers receiver and brain
log actions in the command palette that switch the main panel to a log view.
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

### `workspace/`
The typed, selection-independent workspace foundation. `id` owns the immutable
UUID newtype, `name` validates canonical lower-case slugs, `context` owns an
already-resolved root and the machine's local user ID, and `paths` derives every
machine-local runtime path from the immutable UUID. `registry/` owns the
versioned machine registry, split by responsibility into `model` (schema and
validated mutations), `validate` (pure whole-registry invariants), `select`
(borrowed canonical/default/alias resolution), `store` (the fixed registry
path, loading, transactional updates, and same-directory atomic replacement),
and `migrate` (the one-time flat-env conversion and exact-byte backup).
Root normalization is pure:
it resolves a relative input against an explicit current-directory base and
removes lexical `.` / `..` components without requiring the path to exist or
canonicalizing symlinks. A relative root requires an absolute injected base;
otherwise context construction returns a typed error rather than storing a
relative root.

The sole machine-global registry is `$XDG_CONFIG_HOME/brain/env.json`, falling
back to `~/.config/brain/env.json`. Its schema version is exactly `2`; every
canonical workspace key owns one complete, siloed `WorkspaceRecord` (UUID,
root, aliases, local user identity, receiver switch, and machine environment
map). Selection borrows exactly that record and never copies or merges another
workspace's environment. Whole-registry validation rejects ambiguous selectors,
duplicate UUIDs, a missing default, and exact or ancestor/descendant root
overlap after absolute lexical normalization. Store transactions clone and
mutate a candidate, validate the whole candidate, atomically persist it, and
only then replace the live value.

At startup, `registry::migrate` checks this fixed file before ordinary command
dispatch. A valid schema-v2 registry is returned without any write. Otherwise
it converts the legacy flat object into exactly one default record, resolving
the root from flat `root`, then the read-only legacy pointer, then
`<home>/brain`; the result is tilde-expanded and lexically normalized without
requiring the directory to exist. The new record receives one UUID, no aliases,
an empty local-user placeholder, a de-duplicated receiver switch, and all other
machine-local flat values inside its `env`. Access policy is deliberately not
machine-local; migration reports that portable setup remains required for a
later readiness layer.

Before replacing an existing flat file, migration creates an adjacent
exact-byte `env.json.legacy-backup` (or the first free numeric suffix), then
uses the atomic registry store. Re-running sees schema v2 and preserves both the
UUID and registry bytes without another backup. The existing env/root callers
currently use a default-record compatibility view so commands continue working;
explicit workspace CLI selection and readiness are not wired yet.

Deserialization has a single trusted boundary: JSON first enters a private raw
schema DTO, then conversion runs the same pure whole-registry validator used by
mutations. Public `Deserialize<MachineRegistry>` uses that conversion, so a
successfully deserialized value is fully valid. `RegistryStore` parses the raw
DTO itself so structural JSON failures retain operation and path context while
domain failures retain their typed `RegistryError` variants. Storage failures
likewise retain their operation, primary and related paths, IO error kind, and
message.

After selection, later layers construct one `WorkspaceContext` before passing
it to workspace-aware commands. Context fields are private and accessors expose
only immutable views, so callers cannot desynchronize the workspace UUID from
its UUID-derived runtime paths.

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

**The auto-sync trigger layer** (`lock.rs`/`watch.rs`/`trigger.rs`, wired
into the shell lifecycle and receiver dispatch) makes sync automatic while keeping the pure/impure
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
  platform watcher, the mpsc event channel, and the debounce loop) and
  `spawn_watcher` (the real auto-sync watcher). `WatcherHandle` stops the thread
  on drop (drop the `Watcher` → the channel disconnects → the loop exits; no
  join, so teardown never blocks). On fire it spawns a detached
  `Direction::Push` run. That direction uses a one-way, non-deleting rclone
  copy; its CSV/counter pass reads remote state only to build a safe upload and
  never writes local state, so the push cannot re-arm its own watcher.
- `trigger.rs` — the single shell-facing entry point: `spawn_detached_sync(dir)`
  spawns the current exe as `brain sync [--pull|--push] --if-idle`, fully
  detached (`process_group(0)` + null stdio). Automatic startup, watcher, and
  receiver-freshness triggers go through it, for two reasons: a sync in a
  separate process can never write over the TUI, and a detached child in its own
  process group outlives the shell / terminal close. `--if-idle` makes a
  redundant trigger coalesce (exit silently) rather than follow. There is no
  in-process sync path anymore (the old `run_locked_sync`/`sync_in_background`
  are gone). The parent moves each `Child` into a small waiter thread so a
  completed background sync is reaped and cannot accumulate as a zombie.
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
- `freshness.rs` — the pure two-hour threshold for deciding whether a receiver
  message needs a downstream pull. `journal::latest_downstream_completion`
  deliberately ignores push-only and aborted rows.
- `config.rs` carries `debounce_ms` (default 3000) and
  `debounce() -> Duration`; `command::format_triggers` renders the startup,
  change-push, and message-pull policies in `brain sync status`.

**The `run_tui` lifecycle seam** (`src/tui/event_loop/setup.rs`) is the one wire
point: after the startup work and before the event loop it calls
`trigger::spawn_detached_sync(Pull)` whenever sync is configured and holds a
`watch::spawn_watcher` handle (when `watch_effective()`). It drops the watcher
after the event loop and performs no exit sync. `tui/app_sync.rs` owns the
receiver freshness gate and the 250ms TUI status poll. It queues stale inbound
work behind a pull and reloads tasks before dispatch. All paths are gated and
best-effort; an unconfigured brain gets no watcher or automatic sync.

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
(`commands`/`triage`), and `tests/` (split by area). `app_brain.rs` owns the
main persistent session; `app_triage_tab.rs` owns the ephemeral daily-triage
tab (open/close/select, the `BrainTab` resolution, and the `tick_triage_done`
auto-close). The overlay-modal state
structs (`PaletteState`, `ConfirmState`, `BrainInputState`, `HelpState`,
`LinkPickerState`, and the confirm enums) live in `modal_state.rs` with
`pub(super)` fields; `mod.rs` keeps only the `App` shell type, `Panel`,
`filter_tasks`, and the module wiring. `status_warning.rs` validates receiver
phone configuration and renders persistent warning content independently from
the transient palette flash.

### Startup (`run_tui`)
`run_tui()` opens the state DB, builds the brain-search picker
(`build_search`), constructs the `App`, then `open_or_focus_brain(None)` spawns
the initial `claude` PTY (resume-vs-fresh) and `focus_tasks()` returns focus to
the tasks main view so `j`/`k` work at once. It then wires the auto-sync
triggers (a mandatory detached pull-biased startup sync and, when
`watch_effective()`, a held `watch::spawn_watcher` handle), runs the event
loop, then drops the watcher and releases the session lock. No exit sync or
idle timer exists. The **daily-triage nudge**
is coupled to that startup sync: when a configured startup sync is pending, `run_tui`
does *not* run the check immediately — it captures the sync journal's latest row
id, kicks the sync, and calls `App::arm_triage_gate` (deferral, no modal). Each
event-loop tick then calls `App::tick_triage_gate`, which — once a newer journal
row appears (the sync finished) — reloads
the synced CSVs and runs `check_daily_triage` exactly once, so the modal
reflects post-sync completion state (pure `triage_gate_resolved` decides
resolution). With no startup sync, the check runs immediately as before. The
brain
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
their own flags, and brain never depends on a shell alias. `env_for_triage` is
the ephemeral-session counterpart: it injects only `BRAIN_TRIAGE_DONE_URL` /
`BRAIN_TRIAGE_TOKEN` and deliberately omits the tracking vars, so the
daily-triage tab stays out of the session DB.

### `triage_signal.rs`
The on-disk bridge for the daily-triage tab's completion signal. Pure
`parse_token` plus a thin file shell (`record_done` / `read_token` / `clear`,
`~/.cache/brain/triage-done.json`): the brain server writes it from
`POST /triage/done`, the TUI polls it each tick. See
[integrations.md](integrations.md).

### `state.rs`
The SQLite state layer (`rusqlite`, WAL) at `~/.cache/brain/state.db`.
`brain_sessions` tracks every Claude session brain launched/adopted with a
`locked_pid` lock; `meta` stores the `panel_side` layout preference. The
resume model is **lock + recency** (`reap_dead_locks`, `pick_resume`,
`claim`, `register_fresh`, `release`). The `PanelSide` enum lives here since
it's the persisted value. Mirrors `tasks/src/state`. See
[data-model.md](data-model.md) and [integrations.md](integrations.md).

### `server/`
Brain has two separate HTTP services. The habits server remains local-only and
serves the habits frontend. The receiver server is a TUI-owned, opt-in
listener on `/sms` and `/email`; it is never started by ordinary TUI startup
and cannot outlive the interactive shell.
- `server/router.rs` — pure route mapping for `/habits`, `/habits/done`,
  `/triage/done`, `/sms`, and `/email`. The former `/webhooks/capture`
  placeholder is removed.
- `server/receiver.rs` + `server/receiver/` — the receiver facade and its
  single-responsibility modules: `http/` owns the bounded four-worker
  `tiny_http` listener and channel queue, `http/sms.rs` and `http/email.rs`
  own provider parsing, `attachments.rs` stages media, and `control.rs` owns
  the protected local command socket.
- `server/security.rs` — pure Twilio HMAC, Resend/Svix HMAC, and exact
  allowlist decisions, including E.164 phone-number validation.
- `server/lifecycle.rs` — the legacy local habits-server lifecycle.
- `server/routes/habits/` — the habits MVC route and embedded frontend.
- `server/routes/triage/` — the `POST /triage/done` controller: the ephemeral
  daily-triage session's completion signal (see `triage_signal.rs`).

`brain --with-receiver` starts the receiver listener after the TUI singleton is
acquired. The global palette can start, stop, restart, and inspect it while
the TUI is alive. Inbound work is queued into a bounded in-memory channel and
is never allowed to interrupt an active agent turn. `tui/receiver_state.rs`
distinguishes a submitted turn from an idle open PTY, so an idle startup panel
can switch to the receiver session even when a modal is on screen. It also
distinguishes active receiver work from a three-minute warm channel lease:
interactive Stop-hook completions are still polled, a same-channel message
reuses the warm PTY, and another channel replaces it only after work finishes.
`tui/app_sync.rs` holds inbound dispatch behind a pull when downstream state is
more than two hours old and exposes current sync state to the footer and
palette. Failed PTY launches retain the message for a backoff retry. Provider replies are handed
to the bounded background worker in `server/delivery.rs`, keeping network
latency off the TUI event loop. The listener rejects
bodies over 1 MiB, uses a fixed worker pool so one slow provider call cannot
block every route, treats idle time as normal, and uses
`tiny_http::unblock()` only for graceful shutdown.

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
- `uuid` (`v4`) — per-shell brain-instance ids, fresh session ids, and
  immutable workspace ids.
- `include_dir` — embeds the repo's `skills/` dir (SKILL.md + scripts) into the
  binary so a public cloner needs no repo checkout; `brain skills sync` writes
  them out. Multi-file skill assets rule out `include_str!`.
- `tiny_http` — the two small synchronous HTTP services under `src/server/`:
  the local habits daemon and the TUI-owned receiver. The receiver uses a
  fixed four-worker pool, bounded request bodies, and a bounded handoff queue;
  this preserves concurrency and backpressure without pulling a Tokio runtime
  into an otherwise synchronous CLI.
- `notify` (8.x) — cross-platform filesystem observation for the **C4
  auto-sync watcher** (`src/sync/watch.rs`). Linux uses the recommended native
  backend. macOS uses notify's one-second `PollWatcher`, because FSEvents can
  silently omit valid changes; this was reproduced by the real-filesystem
  watcher integration test. All decision logic remains in the pure,
  clock-injected `watch::Debouncer`, so we depend on neither
  `notify-debouncer-full` nor `notify-debouncer-mini`.

`brain sync` also depends on **`rclone`**, but as an external command it
shells out to (`src/sync/run.rs`), not a Cargo crate: brain builds the argv
and an env-var-only remote config and lets the user's own `rclone` install do
the transfer. It's a soft prerequisite (checked only when `brain sync` runs;
see [integrations.md](integrations.md)), unlike the hard `markdown-to-pdf`
gate.
