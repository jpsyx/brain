# Features

`brain` is the central terminal dispatch for the second brain and task
system. Everything below is reachable from the persistent shell (bare
`brain`), as a subcommand, or from a command palette.

## The merged shell: two main views + one brain panel

Bare `brain` (and `brain tasks …`) opens a persistent shell with **two main
views** and one app-level **brain panel** (see [glossary.md](glossary.md)):

- **Tasks view** — the startup default. The task-management surface over
  `~/brain/tasks/{tasks,habits}.csv`: tabbed sub-views (`today`, `mit`,
  `past_due`, `week`, `habits`, `backlog`, `all`; `Tab`/`Shift+Tab` cycle
  them), vim navigation, notes expand/render, mark-complete / remove / defer /
  open-links, agenda (`Ctrl+A`), and the daily-triage startup nudge. The full
  key list is in [keybindings.md](keybindings.md).
- **Brain-directory (search) view** — the fuzzy search across projects,
  areas, resources, and archive (the picker described later in this doc);
  formerly what bare `brain` opened.
- **Brain panel** — a live, interactive `claude` session in an embedded PTY,
  open at startup and shared by both main views. It does not belong to either
  view: switching views leaves it open; closing it (`Ctrl+X`, or claude
  exiting) makes the active main view full-width.

Switch main views with `Ctrl+L`/`Ctrl+H` (cycle) or `Ctrl+T` (tasks) /
`Ctrl+B` (brain directory). `Alt+S` opens the keyboard-shortcuts help modal
from either view (the compact footer's `Alt+S  all shortcuts` hint points at
it). Startup focuses the tasks view with the brain panel open but unfocused.

The rest of this document describes the brain-directory view's picker + brain
panel in detail; the tasks view's behavior mirrors the pre-merge `tasks`
shell.

## The brain panel and the search view

`Alt+H` focuses the left panel and `Alt+L` the right (vim-style, spatial);
the focused panel's border brightens and the unfocused one dims. The shell
starts focused on the search panel so a query can be typed immediately; the
brain panel is still spawned at startup with the resumed conversation ready
one `Alt+`-switch away. `Alt+U` / `Alt+D` scroll the focused panel a half-page
up / down
(the brain panel by half its visible rows, the search panel by a page of its
match list) — a keyboard-only alternative to the wheel that fires even while
Claude has focus or the filter is being typed.

**Closing vs quitting.** Exiting claude (e.g. `Ctrl-C` to end the turn, then
`Ctrl-C` again to exit claude) **closes the brain panel** — search goes
full-width and the shell keeps running. It does *not* quit `brain`. To
**re-open** the panel, run **Message brain** (`Ctrl-M`, or the palette row —
which only appears while the panel is closed); it resumes your latest
session. To **quit `brain`** entirely, press `Esc` or `Ctrl-c` from the
**search** panel.

**Start a new session.** `Ctrl+N` starts a fresh Claude conversation in the
brain panel: it sends `/new` + Enter to the running `claude` for you. It fires
from either panel while the panel is open (no need to focus the brain panel
first). When the panel is closed, `Ctrl+N` keeps its search meaning (move the
selection down).

**Session resume.** On startup the brain panel resumes your **most recent
Claude session** — the continuous conversation picks up where it left off.
If you type `/new` (or `/clear`) inside claude — or press `Ctrl+N` — that
fresh session becomes the one brain resumes next time. Running `brain` in a second terminal does
**not** reuse the session a live `brain` already holds (no tangled threads):
it resumes the next-most-recent free session, or starts fresh. If the
session it would resume has no transcript yet (you opened brain last time but
never sent a message), it can't be resumed — brain starts a fresh chat and
says so in the status line. See [integrations.md](integrations.md) and
[data-model.md](data-model.md) for the lock-and-recency model.

**Swap the layout.** The palette's "Move brain panel to the left/right"
command flips which side the brain panel sits on; the choice is persisted
(`~/.cache/brain/state.db`), so it sticks across runs. `Alt+H`/`Alt+L`
follow the new layout (always left/right).

**Opening files never closes the shell.** Selecting a file in search opens
it without tearing the shell down (see "The fuzzy picker" below): text files
open in a **new iTerm2 tab**, everything else hands off to the system
`open`. You stay in brain.

## The command palette (`Ctrl-p`)

The full list of things `brain` can do lives in the **command palette**,
opened with `Ctrl-p`. It opens as a **modal overlay** on top of the current
search, so pressing `Esc` (or `Ctrl-c`) just closes it and drops you back
where you were — it does **not** exit `brain`. Its rows, in order (rows with
a direct keystroke show it dimmed in `[…]`):

- **Create PDF for '<file>'** `[^G]` — convert the highlighted markdown file
  to a colocated same-name PDF and open it. **Shown only when a `.md` file is
  highlighted**, where it leads the list (default-selected) so the palette
  opens ready to run it; a long filename is elided in the label
  (`Create PDF for 'really-long-na...md'`). See "Create a PDF from markdown"
  below.
- **Open file '<file>'** `[↵]` — open the highlighted file (text → a new
  iTerm2 tab / `$EDITOR`; blob → system `open`), exactly what plain `Enter`
  does in the picker. **Shown only when a file is highlighted** (a directory
  has no file to open); a long filename is elided head+tail like the PDF row.
- **Open dir '<dir>'** `[^↵]` — reveal the highlighted entry's directory
  in Finder (a file → its parent dir, a directory → itself), exactly what
  `Ctrl-Enter` does. **Shown whenever an entry is highlighted.** The label
  never shows the absolute path or the filename: it leads with the bucket
  category (`projects/`, `areas/`, `resources/`, `archive/`) and, when too
  long, elides the *middle* keeping the tail (`resources/.../final/parts`).
1. **Message brain** `[^M]` — open the brain panel (resume your latest
   session), or focus it if already open. Shown **only while the panel is
   closed**.
2. **Open tasks** `[^T]` — switch to the tasks main view (task management,
   agenda, triage), in-process.
3. **Search projects** — rescope search to `~/brain/projects`.
4. **Search areas** — rescope search to `~/brain/areas`.
5. **Search resources** — rescope search to `~/brain/resources`.
6. **Search archive** — rescope search to `~/brain/archive` (retired material).
7. **Global search** — search across projects, areas, resources, and archive.
8. **Move brain panel to the left / right** — swap the layout (label names
   the direction the panel would move).
- **Delete '<file>'** `[^D]` — move the highlighted entry (file **or**
  directory) to the Trash. **Shown whenever something is highlighted**, and it
  **trails** the list (never default-selected) so a stray `Enter` on open
  can't delete; the label is elided with the same threshold as the PDF row.
  See "Delete an entry" below.

The search rows rescope the left panel **in place**; "Open tasks" switches
to the tasks main view in-process (the same as `Ctrl-T`). The keystrokes
(`Ctrl-g`, `Ctrl-d`, `Ctrl-m`, `Ctrl-t`) also fire directly without opening
the palette first — `Ctrl-g` and `Ctrl-d` open a confirmation modal (see
below); the rest run their action. `Ctrl-R` **refreshes** the search list
(re-walks the current scope, keeping the query); the list also auto-refreshes
after a PDF is created or an entry is deleted, so the change shows without a
manual refresh.

The palette is a filterable text input: typing narrows the rows. Each
row's matchable text includes its 1-based number, so you can type a digit
(`6`), any word from the label (`message`), or several words (`search
projects`) and the list narrows to the hits. Navigate the filtered list
with ↑/↓, `Ctrl-k`/`Ctrl-j`, or `Ctrl-p`/`Ctrl-n`;
`Backspace`/`Ctrl-u`/`Ctrl-w` edit the query; `Enter` runs the highlighted
row (and exits the picker into that action); `Esc` or `Ctrl-c` closes the
overlay and returns to the underlying search (no cd, no claude, no error).

## Subcommands

There are only two subcommands; everything else lives inside the persistent
shell. Bare `brain` (no subcommand) opens the shell on the tasks view.

| Command | Behavior |
| --- | --- |
| `brain` | Open the persistent shell on the tasks view (the startup default). |
| `brain tasks [view/date/query] [flags]` | Open the shell on the given tasks view/selector/search. |
| `brain tasks --no-tui …` | Print the resolved task list as plain text (no TUI). |
| `brain tasks complete <id>` | Mark a task or habit complete natively, no TUI. |
| `brain tasks doctor` | Run the state/hook health check, no TUI. |
| `brain tasks search <q>` | Open the shell with an initial search over all tasks. |
| `brain config [list\|get\|set]` | Read or change persistent, portable config (see below). |
| `brain env [list\|get\|set]` | Read or change your machine-local brain env: `root`, `markdown_to_pdf_path`, and the Backblaze `sync` block (see below). |
| `brain sync [--push\|--pull] {setup\|init\|status\|conflicts\|resolve}` | Manually sync `~/brain` to a private Backblaze B2 bucket via `rclone bisync` (see below). Opt-in: does nothing until `brain sync setup` configures it. `conflicts` takes `--json` for structured output; `resolve <original>...` deletes resolved conflict copies. |
| `brain check` | Read-only report of pending sync changes (what a `brain sync` would push/pull), via dry-run `rclone bisync` plus task/habit CSV baseline diffs (see below). |
| `brain personalize [show\|get\|set\|edit]` | Read or change your personalization (identity + tag styles). Bare `brain personalize` runs first-run onboarding if nothing is set, else shows current values (see below). |
| `brain skills sync [--root <dir>]` | Render + install the bundled skills into the agent registry (`~/.agents/skills`) and fan out to the frontends (Claude, Codex, OpenCode, Cursor). `--root` installs under a sandbox dir instead of your real setup (see below). |
| `brain server {start\|status\|kill}` | Manage the background brain server, a local-only HTTP daemon shared across all `brain` invocations (see below). |

`brain tasks mark <id> [as] done` is rewritten to `brain tasks complete <id>`
before clap parses it.

### `brain config`

Reads and writes brain's persistent, **portable** config
(`<brain-root>/.config/config.json`) — the values that are right on every
machine (Linear workspace, triage settings, the calendar id, `claude_cmd`,
…). Rides whatever syncs the brain directory.

- `brain config list` (or bare `brain config`) — aligned table of every
  variable, its effective value, and its description.
- `brain config get <name>` — the effective value of one variable.
- `brain config set <name>=<value>` — set and persist a variable (unknown
  names rejected).

`config` runs before the `markdown-to-pdf` prerequisite gate, so it always
works even when that tool is missing. See [config.md](config.md) for the schema
and the prerequisite/auto-discovery rules.

### `brain env`

Reads and writes your **machine-local** brain env
(`~/.config/brain/env.json`) — values that would be *wrong* if copied to
another machine: `root` (where your brain lives on this machine),
`markdown_to_pdf_path` (a machine-specific binary path, auto-discovered and
self-healing), and the Backblaze `sync` block (written by `brain sync setup`,
below — see [config.md](config.md) for its fields). Mirrors `brain
config` exactly, over the env store instead:

- `brain env list` (or bare `brain env`) — aligned table of every env
  variable, its effective value, and its description.
- `brain env get <name>` — the effective value of one variable.
- `brain env set <name>=<value>` — set and persist a variable (unknown
  names rejected).

`env`, like `config`, runs before the `markdown-to-pdf` prerequisite gate.
`~/.config/brain/env.json` is never Backblaze-synced (it lives outside the
brain root on purpose); a legacy `~/.config/brain-root` pointer file is read
for back-compat and auto-migrated into the `root` key on first run. See
[config.md](config.md) for the full store/schema description and
[data-model.md](data-model.md) for the `sync` block's fields.

### `brain sync`

Manual, bidirectional cross-machine sync of the brain directory
(`brain_root()`) to a private Backblaze B2 bucket, via `rclone bisync`. Sync
is **opt-in**: with no configured `sync` block (see [config.md](config.md)),
`brain sync` prints "sync is not configured — run `brain sync setup`" and does
nothing.

- `brain sync` (bare) — bidirectional sync; a same-file conflict is resolved
  by newest edit.
- `brain sync --push` — biases this run local-wins on a same-file conflict.
- `brain sync --pull` — biases this run remote-wins on a same-file conflict.
- `brain sync setup` — a guided walkthrough. It first asks *"do you already have
  a Backblaze private bucket to connect to?"*; answering no prints a step-by-step
  guide to creating one (private bucket, Default Encryption **enabled**, Object
  Lock **disabled**, and a bucket-scoped application key) and waits for you before
  continuing. Then it collects the B2 bucket + credentials (writes the `sync`
  block into **brain env**, not brain config — see [config.md](config.md)),
  verifies or creates the bucket, creates the `RCLONE_TEST` check-access marker
  on both sides, and establishes the initial bisync baseline.
- `brain sync init` — (re-)establish the bisync baseline: bootstrap a fresh
  machine, or recover once rclone refuses to sync because one side's listing
  is empty or the check-access marker is missing (see
  [integrations.md](integrations.md)). It recreates the `RCLONE_TEST` marker on
  both sides before the resync. You rarely need to run this by hand anymore —
  see **auto-resume** below.
- `brain sync status` — the last run (from the local sync journal), the
  configured triggers (`on_start`/`on_exit`/`watch`, with the watcher's
  debounce window shown as `(3000ms debounce)`), and the count of open
  conflicts.
- `brain sync conflicts` — list open conflict copies using the same strict
  friendly-name parser as the structured form. `--json` emits those same
  groups as JSON (one object per canonical original, its
  `original_exists` flag, and its `copies` with `host`/`date`/`modified`/
  `bytes`) instead of the themed line-list — meant for agents/skills to
  consume, e.g. the `/second-brain resolve-conflicts` skill.
- `brain sync resolve <original> [...]` — after you've merged a conflict back
  into its canonical file, safely delete that original's leftover conflict
  copies (never the canonical itself). Refuses (and deletes nothing) if the
  named original doesn't exist — merge into it first. Bare `brain sync
  resolve` (no arguments) drops into an interactive picker over the currently
  open conflict groups. Deletion only: it never runs a sync itself.
  - *Caveat:* a conflict copy is recognized purely by its
    `name (conflict <host> <YYYY-MM-DD>).ext` shape, so a genuine file you
    happened to name exactly that way is indistinguishable from a real
    conflict copy — `conflicts`/`resolve` would treat it as one. Don't hand-name
    files in that pattern.

Like `config`/`env`/`personalize`/`skills`, `sync` is dispatched **before**
the `markdown-to-pdf` prerequisite gate, so it always works even when that
tool is missing.

**Live progress.** A running sync is no longer a silent block: rclone's
progress streams to the terminal live, with a one-line update roughly every
10 seconds (files/bytes transferred, percent complete, transfer rate, ETA) —
useful on the first sync of a large brain, which can take a while.

**Automatic sync (start / exit / watcher / idle pull).** On a configured machine you
rarely run `brain sync` by hand: the persistent shell syncs for you, gated by
machine-local brain-env fields (see [config.md](config.md)).

- **On start (`sync.on_start`).** Opening the shell (bare `brain`) kicks a
  background sync so you start on the latest brain. It runs on its own thread
  and never blocks startup: the first frame renders immediately and the sync
  lands whenever it finishes.
- **On exit (`sync.on_exit`).** Quitting the shell spawns a **detached,
  fire-and-forget** `brain sync` child that pushes your last edits without the
  shell ever waiting on the network: quitting is instant and the child finishes
  in the background.
- **Live watcher (`sync.watch`).** While the shell is open, a filesystem
  watcher auto-syncs a few seconds after you stop editing `~/brain` (the
  `debounce_ms` quiescence window, default 3000ms). A burst of edits coalesces
  into a single sync. VCS/cache/OS cruft and existing conflict copies never
  trigger it (it mirrors the bisync exclude set).
- **Idle pull (`sync.idle_pull_secs`).** Optional and off by default. Set a
  positive interval to pull remote changes periodically while the shell stays
  open, so another machine's edits arrive without closing and reopening
  `brain`.

All four reuse the same `sync_once` machinery (so every auto-sync is
journalled exactly like a manual one) and **coalesce** through a machine-wide
lock: concurrent triggers (start + watcher + idle pull + a second shell + a
manual `brain sync`) never run two rclone syncs at once, the extras skip
cleanly. All are best-effort: a held lock, an unconfigured brain, or a spawn
failure is swallowed, so a trigger never crashes or hangs the shell. Set any
boolean flag to `false` to disable that trigger, and leave `idle_pull_secs` at
`0` to disable the timer; with no `sync` block configured at all, nothing
changes (no watcher thread, no timer, no start/exit sync). `brain sync status`
shows the effective trigger state, the debounce window, and the idle-pull
interval.

#### Migrating a machine to sync

A short runbook for bringing sync online: once for a new bucket, then once
per machine you want to join it.

1. **One-time: create the bucket.** Before the first machine can connect,
   someone creates a private Backblaze B2 bucket to sync to — Default
   Encryption **enabled** (B2 manages the keys), Object Lock **disabled**, and
   a bucket-scoped application key. This step happens in the Backblaze
   console, outside brain; if you tell `brain sync setup` (below) you don't
   have a bucket yet, it walks you through creating one and waits for you to
   finish before continuing.
2. **Per machine: `brain sync setup`.** On every machine you want on sync —
   including the one that just made the bucket — run `brain sync setup`. It
   confirms `rclone` is installed, collects the B2 bucket name and the
   application key/keyID (either from a bucket you already have, or the
   walkthrough from step 1), writes the `sync` block into that machine's
   `~/.config/brain/env.json` (see [`brain env`](#brain-env) above — this is
   machine-local and never rides into the bucket itself), and establishes the
   bisync baseline. On a brand-new machine with an empty `~/brain`, that
   initial baseline is effectively a full pull of everything already in the
   bucket.
3. **Verify the triggers.** Run `brain sync status` and confirm it reports
   the effective `on_start`/`on_exit`/`watch`/`idle-pull` triggers and the last run.
   Auto-sync is on by default the moment `brain sync setup` finishes — you
   don't need to flip anything else on.
4. **Env auto-migration.** The legacy `~/.config/brain-root` pointer and
   `config.json`'s `markdown_to_pdf_path` are migrated into
   `~/.config/brain/env.json` on every first launch of the new binary — a
   no-op on a brand-new machine with no legacy pointer or config to migrate —
   see [`brain env`](#brain-env) above. No manual step needed; it happens
   quietly, before `sync setup` even starts.
5. **Confirm it actually works, across two machines:**
   - An edit, add, or delete on machine A shows up on machine B after each
     machine's next `brain sync` (or automatically, once the triggers above
     are live).
   - `tasks.csv` and `habits.csv` merge silently — editing or completing
     different tasks on both machines never leaves behind a `(conflict …)`
     copy of either file.
   - Editing the *same* prose file on both machines at once (before either
     syncs) produces exactly one keep-both `(conflict …)` copy, which
     `/second-brain resolve-conflicts` merges back into the canonical file and
     clears with `brain sync resolve`.

Once these five steps check out, the machine is fully onboarded: sync runs
itself from here via the triggers in the previous section.

**Optional: syncing your own env.json.** `brain` itself doesn't care how
`~/.config/brain/env.json` gets onto a machine — it just reads a standard XDG
config file. If you want to skip re-running `brain sync setup` by hand on
every new machine, you can track that file privately in your own dotfiles
repo (or similar) and let it carry your bucket + credentials across your
machines. This is entirely optional and outside brain's own sync mechanism.

### `brain check`

A read-only report of what a plain `brain sync` would do, without doing it.
Runs the same `rclone bisync` argv as `brain sync` (bare, `Direction::Both`)
but with `--dry-run` appended, so the normal file lane transfers and writes
nothing. Detected file changes are classified and grouped exactly like the
live sync's detection phase.

Because `tasks/tasks.csv` and `tasks/habits.csv` are excluded from bisync,
`check` also performs a read-only CSV pass: it compares the cached CSV
baseline with the local CSV for rows to push, fetches the remote CSV with
`rclone copyto` into a temp file, and compares that remote text with the same
baseline for rows to pull. CSV summaries show row deltas as `+A ~C -D rows`
(added, changed, deleted), and a failed remote CSV fetch becomes a warning
instead of a false clean report. The command never writes local CSVs, remote
CSVs, or baselines.

- Nothing pending on either side: a single `✓ In sync — nothing to push or
  pull.` line.
- Otherwise: a `Changes to push (N):` and/or `Changes to pull (M):` heading
  (only for the side(s) that have pending changes), each followed by grouped
  file summaries (e.g. `2 changes in notes/`) and CSV row summaries
  (e.g. `tasks.csv: +2 ~1 -0 rows`), then a suggestion line naming the right
  follow-up (`brain sync` to push, to pull, or to push and pull).

Like `sync`, `check` is dispatched before the `markdown-to-pdf` prerequisite
gate and needs no configuration beyond what `brain sync setup` already wrote;
run against an unconfigured or baseline-less brain, it prints the same
"not configured" / "no baseline yet" guidance as `brain sync` instead of
erroring.

**Auto-resume (never-miss guarantee).** If a sync is interrupted (Ctrl-C, a
dropped connection, a crash) mid-baseline, brain never reports it as done —
an interrupted or errored run always journals as `needs_attention`/`aborted`,
never `clean`. The *next* plain `brain sync` automatically detects the
incomplete baseline and transparently resumes it (one internal resync retry,
journalled with the note "auto-resumed after interrupted baseline") before
continuing — no need to manually run `brain sync init` first. The guarantee:
every in-scope file is eventually synced; nothing is silently left behind.

**Deletions propagate both ways.** `rclone bisync` mirrors deletes as well as
edits: deleting a file locally removes it from the B2 bucket on the next
sync, and removes it from every other machine the next time *that* machine
syncs. This is guarded by the `--max-delete` safety check (see
[integrations.md](integrations.md)) so a wiped or never-initialized side
can't wipe out the other; short of tripping that guard, delete is a real,
bidirectional operation, not just a local one.

**Selective sync (optional, off by default).** `sync.exclude` (extra rclone
exclude patterns) and `sync.max_size` (skip files above a size cap) let you
keep large or unwanted paths out of the bucket entirely — e.g. bulky media or
scratch data under a resources directory. Both default to empty, so an
unconfigured brain syncs everything, unchanged from before; see
[config.md](config.md) / [data-model.md](data-model.md) for the fields.

**Keep-both conflicts.** When the same file changed on both sides, rclone
doesn't drop the losing edit: it keeps that copy, and brain renames it to
`name (conflict <host> <date>).ext` alongside the winner, so nothing is
silently lost. Conflict copies are themselves excluded from sync, so they
don't fan out to every machine. `brain sync conflicts` lists them (`--json`
for the structured, agent-consumable form); the `/second-brain
resolve-conflicts` skill reads that JSON, merges each group into its
canonical file, then clears the copies with `brain sync resolve <original>`.

**Task CSVs merge by id — no conflict copies.** `tasks/tasks.csv` and
`tasks/habits.csv` don't go through the keep-both path above at all: brain
excludes them from the bisync file lane and reconciles them itself with an
id-keyed 3-way merge (a cached local baseline + your local copy + the remote
copy), writing the merged result back to both sides. Two machines that each
add, complete, delete, or edit different fields on the same task converge
cleanly, so neither file ever produces a `(conflict …)` copy. A side that
marks a task `status=done` always wins that row's status and completed date;
a same-field disagreement otherwise resolves by whichever side's
`last_touched` is more recent. Both `tasks.csv` and `habits.csv` carry that
column; legacy rows without a parseable timestamp fall back to a deterministic
tiebreak, journalled as a soft conflict. See [data-model.md](data-model.md)
for the merge rules and
[integrations.md](integrations.md) for the transport.

**Doctor.** `brain tasks doctor` reports rclone/sync health as one
informational line: `rclone ✓ <version> · sync configured` or
`rclone ✗ not installed · sync off`. An unconfigured (or rclone-less) sync is
a normal, healthy state — it never fails the doctor check.

### `brain personalize`

Reads and writes your **personalization** — content *about you*, stored beside
the config store at `<brain-root>/.config/personalization.json` (just another
brain config, inside the brain root so it travels with the brain).

- `brain personalize` (bare) — first-run onboarding if nothing is set yet:
  a short, skippable prompt for your name, role, and who you work for, then two
  toggle-checklists for your **project namespaces** and **task tags** (all items
  pre-checked; space toggles, `a` adds comma/semicolon-separated new ones).
  Otherwise prints your current values (same as `show`).
- `brain personalize show` — a stable, keyed block (`name:` / `role:` /
  `works_for:` / `namespaces:`) that brain skills read at runtime to learn who
  they're assisting and which project namespaces exist. `namespaces:` shows the
  effective set (your list, or the generic defaults when unset).
- `brain personalize get <field>` — one field (`name`, `role`, `works_for`).
- `brain personalize set <field>=<value>` — set and persist an identity field.
- `brain personalize edit` — open the raw JSON in `$EDITOR` (edit tag-style
  emoji/labels here; the tag and namespace *sets* are edited with the checklist
  via `brain config set tags|namespaces`).

**Tag styles.** The task renderer's tag → emoji+label mapping is personalization.
The binary ships only a tiny universal default set (`mit`, `personal`, `work`);
any other tag renders as its raw name until you add a style under `tag_styles`
in the personalization JSON. So the public binary carries no personal taxonomy.

**Every mutation re-renders skills.** `personalize set`/`edit`, first-run
onboarding, and `config set` all trigger a skill re-render so the installed
skills never drift from your values. (The render pipeline itself lands in a
later sub-project; the trigger is wired now.)

Like `config`, `personalize` runs before the `markdown-to-pdf` prerequisite
gate, so it always works. See [config.md](config.md) and
[data-model.md](data-model.md) for the store layout and schema.

### `brain skills`

Manages the **bundled brain skills** — the skills that ship with brain and
install into the shared agent registry so they work in *any* Claude (or Codex,
OpenCode, Cursor) session, not just inside brain.

- `brain skills sync` — render each bundled skill and install it: write a built
  copy, link `~/.agents/skills/<name>` at it, then link each frontend's skills
  dir at the registry entry. Idempotent; re-run any time.
- `brain skills sync --root <dir>` — install everything under a **sandbox** dir
  instead of your real `~/.agents`/frontend dirs. Used for testing so a run
  never disturbs your live setup.

The skills are embedded in the binary, so a fresh clone needs no extra files.
Installing is also triggered automatically after a `config`/`personalize` change
when `skills_auto_sync` is `true` (the default since the B4 cutover; set it
`false` to sync only on demand). Bundled today: `article-summarizer`, `triage`,
`brain-knowledge-capture`, `second-brain`, `contacts`, and `todo`. See
[config.md](config.md) and the sub-project B spec.

**Customizing skills without forking.** Two mechanisms, both stored with your
brain (synced, never committed to the repo):

- **Extensions** — personalize a *bundled* skill without a new skill. Put a
  `<root>/.config/extensions/<skill>.md` file with `[hook]` sections; the sync
  injects each hook's text at the skill's matching `<!-- brain:ext hook -->`
  marker in the **built copy** (the repo skill is never touched). Content with no
  matching marker is appended as a "Personal extensions" section, so nothing is
  lost. This is how, e.g., the bundled `triage` skill declares
  `triage:daily-open` / `triage:daily-linear` / `triage:weekly-inboxes` /
  `triage:weekly-linear` hooks so a personal extension can bolt an email pass,
  an issue-tracker reconcile, and a cloud in-basket onto the generic core.
- **Plugins** — whole skills you own, in `<root>/.config/plugins/<name>/`. The
  sync installs them alongside the bundled cores, into the same registry and
  frontends.

### `brain server`

The **brain server** is a small, local-only HTTP daemon: one shared instance
per machine, reused across every `brain` invocation and tab. It is a general,
growable localhost service. `GET /habits` renders today's habits as a
flat-design HTML page (grouped by time-of-day, then priority, with a completed
accordion), and `POST /habits/done` marks a habit done by delegating to brain's
native completion machinery, so the web "done" spawns habit recurrence exactly
like the CLI and returns `{"ok": true, "next_due": <date|null>}`.
`POST /webhooks/capture` captures any non-empty request body under
`<brain-root>/scratch/webhooks/` and returns
`{"ok": true, "path": "scratch/webhooks/<timestamp>-<seq>.<json|txt>"}` with
HTTP 202, giving local webhook relays a generic inbox without vendor-specific
schema. Empty capture bodies return HTTP 400. Everything else, including the
bare root `/`, is a 404 (the server has no root view).

- `brain server start` — start the daemon in the background if it isn't already
  running (idempotent: an existing live server is reused and its URL reprinted).
- `brain server status` — report whether it is running and on which port.
- `brain server kill` — stop the background server and drop its record.
- `brain server run --port <p>` — the internal blocking accept loop the spawned
  daemon runs; hidden from `--help` (you never invoke it directly).

The daemon prefers port `8787`, falling back to an OS-assigned port if it's
taken, and records its `{pid, port}` at `~/.cache/brain/server.json`. Opening
the shell (`brain` / `brain tasks`) best-effort brings the server up, so it is
normally already running. A server failure never blocks the shell. The server
binds only to `127.0.0.1`; exposing `/webhooks/capture` through a public tunnel
requires an auth layer outside this slice.

### Prerequisite: `markdown-to-pdf`

Every command except `brain config`, `brain env`, and `brain sync` fails fast with a red `❌`
error if the `markdown-to-pdf` command can't be resolved (it's needed for
"Create PDF"). Its path is auto-discovered on first run and stored as
`markdown_to_pdf_path` **in brain env** (`~/.config/brain/env.json`, not
`config.json`); see [config.md](config.md).

## The fuzzy picker

The search panel of the persistent shell (the brain-directory main view). It
collects entries under the bucket roots and renders a filterable, grouped
list.

- **Typing** filters live. Matching is substring-based: every
  whitespace-separated word in the query must appear as a contiguous run
  in the entry. Slug separators (`-`, `_`, `.`) are stripped before
  matching, so `afloat` finds `ann-afloat` and `annafloat` and
  `ann afloat` all hit. See [data-model.md](data-model.md).
- **Grouping**: matches are grouped under section headers (Projects →
  Areas → Resources → Archive) showing a per-section count. Headers occupy
  a row but aren't selectable.
- **Highlights**: matched characters are colored; the highlight offsets
  are mapped back from the normalized string to the original display
  bytes so they line up exactly.
- **Selecting**:
  - `Enter` → **open directly, in place**. Text-like files open in a **new
    iTerm2 tab** (cd'd to the file's directory, then `$VISUAL`/`$EDITOR`/`nvim`);
    everything else hands off to the system `open`; a directory reveals in
    Finder. The brain shell stays open in all cases.
  - `Ctrl-Enter` → **reveal in Finder**. Files resolve to their parent
    directory.
- **Command palette**: `Ctrl-p` opens the top-level command palette (the
  menu) as a modal overlay for any action `brain` can run; `Esc` closes it
  back to the picker.
- **Cancel**: `Esc` / `Ctrl-c` exits with no action.

See [keybindings.md](keybindings.md) for the complete key table including
movement, paging, and query editing (`Ctrl-u`, `Ctrl-w`, Backspace).

## Create a PDF from markdown

When the highlighted entry is a markdown file (a name ending in `.md`), two
extra affordances appear:

- The **command palette** grows a leading **"Create PDF for '<file>'"** row
  (`[^G]`), default-selected so `Ctrl-p` → `Enter` runs it immediately.
- **`Ctrl-G`** fires it directly, first raising a small green **yes/no
  confirmation modal** ("Would you like to create a PDF for '<file>'?").
  `Enter`/`y` confirm, `n`/`Esc` cancel, `←`/`→`/`Tab` toggle the buttons.
  (The palette row skips the modal — choosing it is already a confirmation.)

Either way, `brain` converts the markdown to a **colocated, same-name PDF**
(`plan.md` → `plan.pdf`, in the same directory) using the user's
`markdown-to-pdf` tool, then opens the result with the system `open`. In the
persistent shell this happens **in place** — the brain shell stays up, and the
search list **auto-refreshes** so the new PDF appears immediately. Any
pre-existing PDF at that exact path is replaced so the output name always
matches the source. Only `.md` is offered (the converter's sole input); the
label elides a long filename to fit the palette. See
[integrations.md](integrations.md) for the tool handoff.

## Delete an entry

When any entry is highlighted (a file **or** a directory), two affordances
appear:

- The **command palette** grows a trailing **"Delete '<file>'"** row (`[^D]`).
  It is deliberately last and never default-selected, so a stray `Ctrl-p` →
  `Enter` can't delete.
- **`Ctrl-D`** opens a small **red** yes/no confirmation modal ("Delete
  '<file>'? It moves to the Trash."). Because deleting is destructive the
  modal **defaults to No**: `Enter` cancels, and deleting takes a deliberate
  `y` (or a toggle to Yes first). `n`/`Esc` cancel. The palette's "Delete" row
  routes through the same modal (the guard is never skipped).

Confirming moves the file or directory to the **Trash** (via Finder, so it's a
recoverable, user-style delete — a `Put Back` away, not an `rm`), then the
search list refreshes so the entry disappears. This happens **in place** and
the shell stays up. The label elides a long filename with the same threshold
as the "Create PDF" row.

## Open tasks

`Ctrl-t` (or palette item "Open tasks") switches to the **tasks main view**,
the ratatui task-management surface backed by `~/brain/tasks/{tasks,habits}.csv`
(today / MIT / past-due / week / habits views, agenda, triage). It is the
startup default and `brain tasks` opens straight onto it. The tasks view is
in-process — a main view of the same shell, not a separate binary — so the
switch is instant and the brain panel stays open beside it. See
[integrations.md](integrations.md) for the tasks-view shell-outs
(agenda / habits).

## Help and version

`brain --help` / `brain -h` print the clap-generated usage (with the
long-form command descriptions and the TUI key summary). `brain --version`
prints the crate version. Both are printed by clap straight to stdout.
