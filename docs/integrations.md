# Integrations

`brain` is a single binary that owns its own outside-world effects. It has no
shell-mutating one-shot commands, so there is no plan protocol and no zsh
wrapper: everything the user does happens inside the persistent TUI, which
opens files, reveals in Finder, converts PDFs, trashes entries, and launches
`claude` by spawning processes itself. This doc covers how the binary is run
and each of those handoffs, plus the SessionStart hook and state DB.

## How brain is run (`run.sh`)

`run.sh` is the entry point. It rebuilds `target/release/brain` when
`Cargo.toml` or any `src/**/*.rs` is newer than the binary (build chatter to
stderr), then `exec`s the binary, forwarding every argument. It does **not**
capture stdout, parse a plan, or apply any parent-shell effect — the binary
handles its own effects.

The binary's **stdout** carries only:

- `brain config …` output (the config table, or a single value), and
- clap's help / version / error text.

Everything else is a TUI that renders to `/dev/tty`, so nothing an interactive
session paints reaches stdout. Diagnostics go to stderr.

## The tasks view (in-process, no handoff)

The tasks CSVs (`~/brain/tasks/{tasks,habits}.csv`) are read directly by
`brain`'s tasks main view (`crate::tasks`), and `brain tasks …` launches the
merged shell (or runs a tasks utility) in-process. The tasks-view side effects
that *do* shell out live in the tasks modules:

- **`~/global-skills/todo/scripts/mark_done.py`** — `brain tasks complete <id>`
  and the palette's mark-complete action `exec`/invoke it to mutate the CSVs.
- **`agenda` zsh function** — `Ctrl+A` runs it via the injected `ShellRunner`.
- **`brain habits` / palette "Open habits in browser"** — bring up the bundled
  brain server (`server::lifecycle::ensure_running`) and open its `/habits`
  page via the system `open`; they no longer shell out to a zsh function.
- **`cd <root> && <claude_cmd> …`** — the brain panel's PTY, shared by both
  main views (see below).

This is the "central dispatch" design: `brain` is the single terminal command,
and each capability is either an in-process main view (tasks, brain-directory
search) or a spawned process it drives (claude for conversational work,
Finder/editor for files, `markdown-to-pdf` for conversions).

## The brain panel: claude session + SessionStart hook + state DB

The persistent shell's brain panel spawns `claude` itself, inside a PTY
(`pty_pane.rs`), running `cd <root> && <claude_cmd> --resume <id>` or
`--session-id <id>` (`session::build_claude_command`). `<claude_cmd>` is the
configurable launch command (`config::Config::claude_command`, config variable
`claude_cmd`, default `claude --dangerously-skip-permissions`), spliced in
verbatim so it may carry its own flags; brain always appends the `--resume` /
`--session-id` flag it controls, so it never depends on a shell alias. The
PTY's working directory is **also** set to `<brain_root>` (resolved via
`paths::brain_root()`, honoring the `root` config), so claude resolves its
project dir (and the SessionStart hook in `.claude/settings.json`) under the
brain root from the first instant — every brain session is scoped there, so
resume always looks in the same place.

Which session to run is decided by the **lock + recency** model in
`state.rs` (DB at `~/.cache/brain/state.db`, WAL):

1. On startup brain reaps locks held by dead PIDs, then walks
   `free_sessions_by_recency()` (sessions no live brain holds, newest first)
   and resumes the first whose **transcript actually exists** on disk —
   `~/.claude/projects/<mangled ~/brain>/<id>.jsonl`
   (`session::project_dir_name` + a fallback scan). A session opened but
   never chatted in leaves a DB row with **no** transcript, which `claude
   --resume` can't find (the "couldn't find session with ID …" error); brain
   skips those. If it claims a valid candidate it `--resume`s it; otherwise
   it starts a fresh `--session-id` (registered, locked to this PID) and, if
   it skipped a missing-transcript candidate, shows a status-line alert:
   *"couldn't find a session to resume — starting a new brain chat"*.
2. brain passes `BRAIN_INSTANCE_ID` / `BRAIN_PID` / `BRAIN_STATE_DB` into
   the claude child's environment (`session::env_for`).
3. A **SessionStart hook** —
   `scripts/claude_session_start_hook.py`, wired into
   `~/brain/.claude/settings.json` under `hooks.SessionStart` — fires on
   every session start / resume / `/clear` / compact. Reading those env
   vars, it upserts the current session id (locked to `BRAIN_PID`) and frees
   the instance's other sessions, so a `/new` mid-run becomes the session
   brain resumes next time and the prior conversation stays resumable. With
   the env vars absent (ambient `~/brain` claude usage), the hook is a no-op.
4. When the panel closes (claude exits) or the shell quits, brain `release`s
   its lock, floating that session to the top of the resume queue — so
   "Message brain" (`Ctrl-M`) re-opens it, and a fresh startup resumes it.

**One hook, one DB, one namespace.** Before the merge, `brain` and `tasks`
each ran their own SessionStart hook keyed on separate env-var namespaces
(`BRAIN_*` vs `TASKS_*`) writing separate DBs, so the two shells never adopted
each other's sessions. The merged shell has a single app-level brain panel, so
there is now exactly **one** hook (`scripts/claude_session_start_hook.py`, keyed
on `BRAIN_*`), one DB (`~/.cache/brain/state.db`, table `brain_sessions`), and
one namespace. `scripts/install_hook.sh` installs it and strips the legacy
`tasks` SessionStart hook and the old `claude_stop_hook.py` Stop hook if
present. brain deliberately does **not** add a Stop hook — its brain-panel
sessions are continuous conversations, not discrete runs.

## System `open` and the editor

The search view opens files from inside the running TUI, via `open_target`'s
impure spawners; the brain panel never closes. A text file →
`open_in_editor_tab`, which runs `osascript` to open a **new iTerm2 tab**
(`iterm_new_tab_applescript` over `edit_shell_command` = `cd <dir> &&
${VISUAL:-${EDITOR:-nvim}} <file>`); a blob or directory → `open_with_system`
(`open <path>`); a Finder reveal (`Ctrl-Enter`) resolves to the parent dir
(`open_target::finder_target`) and calls `open` on it. Whether a file is text
or a blob is decided by `open_target::is_textlike`. On a non-iTerm2 terminal
the editor path falls back to `open <file>`. Nothing is emitted to stdout; the
shell stays up throughout.

## Handoff: `markdown-to-pdf` (the "Create PDF" command)

The "Create PDF" command (palette row / `Ctrl-G` on a `.md` file) converts the
highlighted markdown to a colocated same-name PDF and opens it. It reuses the
user's existing converter rather than reimplementing PDF generation.

`markdown-to-pdf` is a hard prerequisite. Its path is the brain-env variable
`markdown_to_pdf_path` (machine-local, set via `brain env`), auto-discovered on
first run and validated at startup (see [config.md](config.md), `src/env/`, and
the gate in `settings/`); a missing/invalid path fails
fast with a red error. `open_target::create_pdf` spawns that command directly
(`<file.md> --out <file.pdf>`) — invoking the command, not any shell-function
wrapper, since a child process can't call a shell function. The output path is
`open_target::pdf_output_path` (same directory, same stem, `.pdf`).

- **Same-name guarantee.** The converter's non-interactive mode does *not*
  overwrite an existing PDF — it writes a `-vN` variant. To keep the output
  name identical to the source, `create_pdf` removes any pre-existing PDF at
  the target path first, so the converter always writes the exact name.
- **Opening the result.** The conversion runs in place and the PDF is handed
  to `open_target::open_with_system` (`open <pdf>`) — the brain shell stays up.
- **Best-effort.** A converter failure is swallowed (like a failed file-open)
  so a broken toolchain can't tear the shell down.

## Handoff: `osascript` → Finder trash (the "Delete" command)

The "Delete" command (palette row / `Ctrl-D` on any entry) moves the
highlighted file or directory to the **Trash** rather than unlinking it, so a
mistaken delete is recoverable (`Put Back`). It's a **user-style** delete: no
new mechanism, just the same Trash the user empties by hand.

`open_target::move_to_trash` shells out to `osascript` with the line
`open_target::trash_applescript` builds — `tell application "Finder" to delete
POSIX file "<path>"` (the path escaped for the AppleScript literal). Finder's
`delete` handles both files and directories and lands them in the Trash.

- **Confirmed first.** Both entry points route through the red `confirm.rs`
  modal (default **No**); the trash only runs on `Accept`.
- **Refresh after.** The search view re-walks its scope (`App::refresh`) and
  drops the trashed path (`picker::App::drop_path`), so the entry disappears
  from the list.
- **Best-effort.** A failed `osascript` is swallowed (like the PDF path) so a
  denied automation permission can't tear the shell down.

## Handoff: `rclone` + Backblaze B2 (`brain sync`)

`brain sync` (`src/sync/`) manually syncs the brain root to a private B2
bucket by shelling out to `rclone bisync`. It's a handoff like
`markdown-to-pdf`: brain doesn't reimplement transfer or conflict resolution,
it drives an existing tool and manages the surrounding safety and
bookkeeping.

- **Credentials never touch argv or disk.** `src/sync/remote.rs`
  (`build_remote`) turns the brain-env `sync` block (`b2_bucket`, `b2_path`,
  `b2_key_id`, `b2_app_key`) into `RCLONE_CONFIG_BRAIN_*` environment
  variables (`_TYPE`/`_ACCOUNT`/`_KEY`) passed to the rclone child process,
  plus a `BRAIN:<bucket>[/<path>]` remote argument that carries no secret.
  There is no persisted `rclone.conf` anywhere: the remote is reconstructed
  from brain env on every invocation, and because credentials ride in the
  child's environment rather than its argv, they never show up in `ps` output.
- **The bisync argv is built once** by `src/sync/args.rs`
  (`bisync_args`): direction (`brain sync` / `--push` / `--pull` / `brain
  sync init`) maps to rclone's `--conflict-resolve` (`newer` / `path1` /
  `path2`), plus `--conflict-loser pathname` + `--conflict-suffix
  __brainconflict__` (the keep-both mechanics — see [features.md](features.md)
  and [data-model.md](data-model.md)), `--max-delete <percent>`, `-v` (so
  rclone emits the `Transferred:`/`Deleted:`/`Errors:` summary block the parser
  reads — at default verbosity rclone prints no summary and every count parses
  as 0), `--stats 10s --stats-one-line` (periodic one-line progress instead of
  rclone's default one-shot summary), `--resilient --recover` (so an
  interrupted run can resume on the next invocation without forcing a full
  `--resync`), `--check-access --check-filename RCLONE_TEST`, and default
  excludes (`.git/**`, `.DS_Store`, `.cache/**`, friendly conflict copies
  `*(conflict *)*`, and raw markers `*.__brainconflict__*`) plus any
  user-configured `sync.exclude` patterns and an optional `sync.max_size` cap
  (`--max-size`, omitted when unset).
  `src/sync/run.rs` (`run_rclone`) spawns `rclone` with that argv and the
  env-var remote, and parses its captured stderr into transferred / deleted /
  error counts plus an abort reason.
- **Progress streams live instead of blocking silently.** `run_rclone`
  inherits its own stdout for the child (`Stdio::inherit()`) and pipes only
  stderr (`Stdio::piped()`) — rclone writes its logs/stats to stderr. That
  pipe is read line-by-line on the main thread: each line is echoed to
  brain's stderr as it arrives *and* appended to a capture buffer, so the
  user watching the terminal sees rclone's live output while brain still gets
  a full transcript to parse into a `RunOutcome` once the child exits. No
  extra thread and no deadlock risk: there's exactly one pipe, and it's
  drained continuously rather than buffered up front. The periodic one-liner
  that makes this worth watching (`--stats 10s --stats-one-line`, e.g.
  `Transferred: 12.3G / 144G, 9%, 5.2 MByte/s, ETA 6h`) comes from
  `args::bisync_args`, alongside `--resilient --recover` (below).
- **`--max-delete` and `--check-access` are both active guards.**
  `max_delete_percent` (from `sync.max_delete_percent` in the brain-env `sync`
  block, default 50) aborts a run that would delete more than that share of
  files, without propagating the deletes. rclone's own safety abort ("too many
  deletes") is mapped by `src/sync/verify.rs` to an `Aborted` outcome pointing
  at `brain sync --resync` (via `brain sync init`) if the deletes were
  intentional. `--check-access --check-filename RCLONE_TEST` is the path
  symmetry guard: rclone aborts unless both sync roots contain the marker.
  `src/sync/check_access.rs` owns that lifecycle. `brain sync setup` and
  `brain sync init` write `<brain-root>/RCLONE_TEST`, copy it to the remote
  root with `rclone copyto`, and then run the resync. Normal `brain sync`,
  `--push`, and `--pull` do not silently repair missing markers; if the guard
  fails, `src/sync/run.rs` classifies it as `AbortKind::CheckAccess` and
  `verify.rs` tells the user to run `brain sync init`.
- **rclone's own empty-directory guard.** Independently of brain's
  `--max-delete` guard, `rclone bisync` refuses to run at all when one side's
  prior listing has gone fully empty ("cannot find prior Path1 or Path2
  listings" / "must run --resync to recover") — its own protection against
  treating a wiped or never-initialized side as "delete everything on the
  other side." `src/sync/run.rs` recognizes this wording as
  `AbortKind::PriorListingMissing`. Historically that meant surfacing a
  pointer at **`brain sync init`** for the human to re-run with `--resync`;
  as of the progress/resume work, `command::sync_once` handles the common
  case (an interrupted or killed `--resync`) itself: `should_auto_resync`
  (pure) says yes whenever the abort is `PriorListingMissing` **and** the run
  that just aborted wasn't already a resync (so it retries exactly once,
  never loops), `sync_once` re-runs bisync as `Direction::Resync`, and the
  journal note records "auto-resumed after interrupted baseline". `brain
  sync init` still exists for a genuinely fresh machine's first baseline and
  as a manual escape hatch, but you no longer have to reach for it after a
  Ctrl-C mid-sync — the next plain `brain sync` resumes on its own.
- **Never journal `clean` for an interrupted or errored run.** This is what
  makes auto-resume safe rather than merely convenient: `verify::classify`
  only ever returns `Clean` on a full, zero-error rclone exit, so an
  interrupted run (even one that transferred most of its files before dying)
  always comes back `NeedsAttention`/`Aborted` and gets auto-resumed (or
  surfaced) on the next invocation — brain never tells you a sync finished
  when it didn't, so nothing in scope is silently left un-synced.
- **Deletions propagate bidirectionally, by design.** `rclone bisync`
  mirrors deletes exactly like edits: removing a file on one machine deletes
  it from the B2 bucket on that machine's next sync, and deletes it from
  every other machine on *that* machine's next sync — there is no
  local-only delete. The only brake on this is the `--max-delete` guard
  above; short of tripping it, a delete is real and bidirectional. B2 itself
  keeps prior file versions after a delete (its own object versioning)
  unless a bucket lifecycle rule is configured to prune them, so a delete
  synced by brain is not necessarily unrecoverable at the B2 layer — but
  brain does not manage or rely on that; treat `--max-delete` as the only
  safety net brain provides.
- **The setup flow.** `brain sync setup` (`src/sync/setup.rs`) checks
  `rclone` is on `PATH`, then acts as a guided walkthrough: it asks whether you
  already have a bucket (`ask_has_bucket` / pure `parse_yes_no`), and if not
  prints `bucket_walkthrough()` — the step-by-step Backblaze bucket + app-key
  guide (private, Default Encryption on, Object Lock off) whose coverage of the
  critical settings is unit-tested — and pauses. It then prompts on `/dev/tty`
  for the bucket + B2 key id + application key (pre-filled with any existing
  values), validates them,
  writes the `sync` block into brain env (`crate::env::set_raw`, **not**
  brain config — see [config.md](config.md)), probes the bucket with `rclone
  lsd` and offers to `rclone mkdir` it if unreachable, then runs one
  `Direction::Resync` sync to establish the baseline. `brain sync init` reruns
  just that last step (the resync), so it doubles as the fresh-machine
  bootstrap and the recovery path for the empty-directory guard above.
- **The journal.** Every run (whichever direction, including `setup`'s
  initial baseline) is classified — `Clean` / `NeedsAttention` / `Aborted` —
  by `src/sync/verify.rs` and recorded by `src/sync/journal.rs` into a SQLite
  journal at **`~/.cache/brain/sync/journal.db`** (table `sync_runs`, WAL like
  the state DB). It's machine-local and **never synced** (it lives outside
  the brain root, like the rest of brain env), so each machine's sync history
  stays its own. `brain sync status` reads the most recent row plus the
  configured trigger flags and the open-conflict count; see
  [data-model.md](data-model.md) for the row schema.
- **Conflicts, on disk.** rclone leaves the losing side of a same-file
  conflict named `<original>.__brainconflict__<N>` (literal dot + suffix +
  trailing integer, e.g. `one.md.__brainconflict__1`), on both sides; a
  post-pass (`src/sync/conflicts.rs`, `rename_markers`, matching via
  `is_marker`) renames it to the friendly `name (conflict <host> <date>).ext`
  right after the rclone run. Both the friendly (`*(conflict *)*`) and raw
  (`*.__brainconflict__*`) patterns are default excludes above, so neither gets
  synced around on a later run. Because the rename leaves zero leftover markers,
  `sync_once` also feeds the *count of copies renamed* into `verify::classify`
  so the run is still reported `NeedsAttention` (journalled `conflicts=N`) — a
  real conflict is never masked as clean. `brain sync conflicts` lists what's
  still open; resolving a group is the agent-driven flow described next.
- **The conflict-resolution contract for agents (C5).** `brain sync conflicts`
  re-derives `ConflictGroup`/`ParsedCopy` from the on-disk friendly names via
  `conflicts::parse_conflict_name` + `conflicts::group_conflicts`, so the
  themed human list and `--json` agree on which files are real conflict copies.
  `--json` is the structured enumerator the `/second-brain resolve-conflicts`
  skill (and any other agent) consumes: `command::conflicts_json` renders each
  group as `{ "original", "original_exists", "copies": [{ "path", "host",
  "date", "modified", "bytes" }] }` (paths relative to the brain root;
  `modified`/`bytes` are `null` when the file's metadata can't be read).
  `brain sync resolve <original> [...]` is the matching brain-side deleter: it
  looks up that original's copies via `conflicts::copies_for_original` and
  deletes them (never the canonical file itself), refusing outright
  (`ResolveDecision::CanonicalMissing` in `src/sync/command/resolve.rs`) if the
  canonical original doesn't exist on disk — the skill must merge into it
  first. `resolve` never invokes `rclone` or the journal; it's a pure local
  filesystem delete, so the skill runs one ordinary `brain sync` afterward to
  push the resolution out.
- **The two task CSVs skip bisync entirely; they're merged out-of-band.**
  `tasks/tasks.csv` and `tasks/habits.csv` are added to `args::bisync_args`'s
  default excludes (`src/sync/args.rs`), so Lane-A bisync never touches them —
  line-based bisync would happily let one machine's edit clobber another's on
  structured, id-keyed data. Instead `command::sync_once` runs a dedicated
  step (`crate::sync::csv_sync::sync_csvs`) once bisync itself hasn't aborted:
  for each CSV it reads the cached baseline (`csv_sync::baseline_path`,
  `~/.cache/brain/sync/baselines/{tasks.csv,habits.csv}`, machine-local and
  never synced), the local file, and the remote copy (fetched with `rclone
  copyto <remote> <tmp>`, over the same env-var `BRAIN:` remote bisync uses);
  merges the three with the pure id-keyed 3-way merge in
  `crate::sync::csv_merge` (`merge(base, ours, theirs)`, keyed by `task_id` —
  see [data-model.md](data-model.md) for the rules); writes the merged CSV
  back to the local file, pushes it to the remote with another `rclone
  copyto`, then overwrites the baseline with the same merged text. A missing
  baseline (first run on a machine) means every row reads as newly added, so
  the first CSV sync is a safe union of both sides rather than a guess. The
  bundled task/habit writers stamp `last_touched` on every row mutation, so
  same-field CSV conflicts normally resolve by row recency on both tables. The
  merge outcome (added/merged/deleted/soft-conflict counts) is folded into the
  sync journal's `note` column as a `csv: +A ~M -D` segment (see
  [data-model.md](data-model.md)); a CSV-merge failure never changes the
  bisync run's own outcome, and the step is skipped entirely when that run
  aborted. See [decisions.md](decisions.md) for why this file pair gets a
  semantic merge instead of keep-both.
- **`brain check` has a read-only CSV lane too.** Since those two CSVs are
  excluded from dry-run bisync, `src/sync/check.rs` reads the same cached
  baselines, reads the local CSVs, fetches each remote CSV with `rclone copyto`
  into a temp file, and reports row-level `+A ~C -D` push/pull deltas. It never
  writes local files, remotes, or baselines; if a remote CSV cannot be fetched,
  the local row diff is still shown and the remote side is reported as
  unchecked.
- **rclone is a soft prerequisite, not a startup gate.** Unlike
  `markdown-to-pdf`, a missing `rclone` never blocks `brain` from starting —
  `brain sync` itself just fails when it tries to spawn `rclone` and can't.
  `brain tasks doctor` (`src/tasks/doctor.rs`) reports rclone/sync health as
  one informational line (`rclone ✓ <version> · sync configured` or `rclone ✗
  not installed · sync off`), which never affects the doctor's overall
  pass/fail.

## Auto-sync triggers (the sync lock, the exit child, the watcher)

The C4 auto-sync layer (`src/sync/{lock,watch,trigger}.rs`, wired into
`src/tui/event_loop/setup.rs`'s `run_tui`) drives the same `rclone` handoff
above automatically. Its own outside-world touchpoints:

- **The machine-wide sync lock** (`src/sync/lock.rs`) is a PID file at
  `~/.cache/brain/sync/sync.lock` (beside the sync journal, machine-local
  cache). It holds the owning process's PID and is taken atomically via
  `create_new` (O_EXCL), so only one sync runs at a time across every trigger
  and every `brain` invocation on the machine (the extras skip rather than
  queue). `Guard` owns a heartbeat thread that refreshes the lockfile mtime
  while the sync is still running. A later acquire reaps the lock when either
  the owner PID is dead (the same `server::lifecycle::pid_alive` `kill -0`
  probe the server uses) or the heartbeat mtime is older than the stale cap;
  that closes the SIGKILL + PID-recycle wedge. `Guard` stops the heartbeat and
  removes the file on drop, but only if it still holds **our** PID, so a Guard
  whose lock was reaped out from under it (a crash-recovery race) never deletes
  the new owner's lock. A missing or garbage lockfile reads as stale/reapable.
  The manual `run_sync` in `main.rs` takes this lock too, closing a pre-existing
  concurrent-`brain sync` race.
- **The detached `on_exit` child** (`trigger::spawn_detached_sync`) spawns the
  current exe as `brain sync` fully detached — `process_group(0)` (its own
  process group, so it outlives the parent) plus stdin/stdout/stderr all set to
  `Stdio::null()` — mirroring how `src/server/lifecycle.rs` spawns the server
  daemon, and needing no `unsafe`. The shell finishes teardown and exits at
  once; the child acquires the sync lock itself and pushes the final state in
  the background (if a sync is already running it skips, since that run covers
  the exit).
- **The watcher's exclude set** (`watch::is_watch_relevant`, a pure path
  predicate) mirrors the bisync filter (see `args::bisync_args`'s default
  excludes above): a changed path under `.git`, `.cache`, or a `.DS_Store`, or
  an existing friendly conflict copy (`*(conflict *)*`), never triggers a sync.
  So a VCS write, a cache churn, or a conflict copy fanning in from another
  machine can't kick the watcher. The watcher runs `notify` recursively over the
  brain root; when relevant changes settle for the `debounce_ms` window it runs
  one locked sync **synchronously in the watcher thread**, so the sync's own
  pull writes buffer in the event channel and coalesce into at most one no-op
  follow-up rather than looping.

## The auto-rebuild

`run.sh` rebuilds `target/release/brain` whenever `Cargo.toml` or any
`src/**/*.rs` is newer than the binary, then `exec`s it. This is the only
reason an agent's source edit "takes effect" without a manual
`cargo build` — but you should still build/test explicitly while
developing (see [testing.md](testing.md)).
