# Decisions

The "why" behind `brain`'s non-obvious choices. Architecture is in
[architecture.md](architecture.md); this file is the rationale an agent
needs before second-guessing a design.

## Why tasks and brain were merged into one CLI

They went hand-in-hand and both already embedded the *same* kind of `claude`
brain panel anchored at `~/brain`, so running two separate shells (each with
its own panel, session DB, `SessionStart` hook, and env namespace) was
duplicative and meant the two panels couldn't share a conversation. Merging
gives one shell with one app-level brain panel shared across a **tasks view**
and a **brain-directory view**.

Decisions taken during the merge (see the conversation that produced it):

- **Tasks view is the startup default**, brain panel open but unfocused so
  `j`/`k` work immediately.
- **Two switching axes, deliberately distinct.** `Ctrl+H/L` (cycle) and
  `Ctrl+T`/`Ctrl+B` (jump) switch *which main view* shows; `Alt+H/L` move
  *panel focus*. View-switch chords are intercepted only when the main panel
  has focus, so the brain panel keeps Claude's readline chords (`Ctrl+H` =
  backspace, etc.) when focused.
- **`Alt+S` (not bare `?`) opens help**, so a literal `?` still types into the
  always-filtering brain-search view. It's a Meta sequence, reliable on every
  terminal.
- **Displaced bindings.** `Ctrl+B` (was brain's "go to root") is repurposed to
  the brain-view jump and go-to-root was **dropped** (redundant now the view
  is one keystroke away in-app). `Ctrl+H` (was tasks' "open habits page") is
  repurposed to cycle-left; opening the habits page moved to the palette.
- **One session namespace.** The separate `BRAIN_*` / `TASKS_*` namespaces and
  DBs existed only to stop two shells adopting each other's sessions; with one
  panel there is one hook, one DB (`brain_sessions`), one `BRAIN_*` namespace.
- **`tasks` utilities nest under `brain tasks …`** (`complete`, `doctor`,
  `search`, `--no-tui`); the old `tasks` binary/command and its `tasks=1` plan
  handoff are gone (cold-turkey, per the user).
- **The tasks tui was the merge base**, not brain's: it was the richer,
  default surface, so the merged `App` is the ported tasks `App` extended with
  a `MainView` axis and brain's `picker` embedded as the second view. brain's
  shared `session`/`state`/`pty_pane` (near-identical ports) were kept
  as the single copy.

## Why `brain` is a "central dispatch", not a single-purpose tool

The user lives in the terminal and wants **one command** to reach everything
inside the selected workspace: manage tasks, jump to a note, search across PARA
buckets, or think with an agent session rooted there. Rather than
memorize several separate entry points, `brain` is the front door: a single
persistent shell with three main views (tasks, brain-directory search, and logs)
and a live brain panel. New top-level capabilities should be added as a
main view, a palette row, or a keybinding inside that shell, not as a separate
command (or a shell-mutating one-shot subcommand) the user has to discover.

## Why the command palette lives behind `Ctrl-p`

Every non-default action (per-bucket search, message brain, open tasks, move
the panel, create a PDF, delete) lives in the **command palette**, reachable
with `Ctrl-p` from the search view, so it is one keystroke from the search box
without cluttering the screen. `Ctrl-p` is the palette hotkey (matching the
`tasks` TUI), which is why it is no longer an up-motion alias in the picker (up
is `↑` / `Ctrl-k`).

## Why the palette is a modal overlay, not its own screen

The palette is drawn as a modal **overlay inside the picker's event loop**
(`menu::draw_modal` over the picker, `menu::MenuApp` driven by the picker's
`handle_key`), rather than a separate full-screen TUI the way it started.
The reason is `Esc`: a separate screen would have to *exit* on `Esc`,
dropping the user all the way back to the shell and losing the search they
were in. As an overlay, `Esc` just closes the box and the picker is still
right there underneath — the same back-out-of-a-modal behavior the `tasks`
TUI has. This is why `menu/` has no `run()`/event loop of its own; it
exposes pure state (`MenuApp`, `handle_key`) plus `draw_modal`, and the
search view owns the loop. A confirmed row returns a `Choice`, which
`tui/search_view.rs` runs in place (rescope, message brain, open tasks,
PDF/delete) without leaving the shell.

## Why archive is browsable now (it wasn't before)

Archive (`~/brain/archive`) is retired PARA material, and originally `brain`
left it out of every search on purpose. In practice the user still needs to
dig things back out of the archive, so it's now a first-class bucket:
"Search archive" is its own palette row and the `Archive` bucket is part of
global search. It sorts **last** in every grouped result so live
Projects/Areas/Resources stay on top and retired material doesn't crowd the
common case. Rescoping to any single bucket (Archive included) is a palette
row in the search view, never a CLI subcommand.

## Why the palette's top rows carry direct keystrokes

The two actions the user reaches for most — message brain, open tasks — sit at
the top of the palette and also fire directly from the search view via
`Ctrl-m` / `Ctrl-t`, so the common cases don't even require opening the
palette. The keystrokes are surfaced as dim `[…]` hints on their palette rows
(the `tasks` convention, via `menu::shortcut_for`) so they're discoverable
without cluttering the layout. `Ctrl-m` shares Enter's byte (`0x0D`), so it
depends on the kitty protocol we already push to stay distinct; without the
protocol it degrades to a plain `Enter` (open the selection), the same safe
fallback as `Ctrl-Enter`.

## Why `Enter` opens and `Ctrl-Enter` reveals (the swap)

Opening the file is the action the user wants most of the time, so it gets
the unmodified `Enter`; revealing the containing directory in Finder is
the rarer case and takes the `Ctrl-Enter` chord. A directory match has no
file to open, so `Enter` on one falls back to revealing the directory
(identical to `Ctrl-Enter`), which keeps `Enter` from ever being a no-op.

## Why `brain` needs no plan or wrapper

`brain` used to have shell-mutating one-shot subcommands (`cd`, `msg`, the
per-bucket search verbs). Those effects had to happen in the **parent shell**
(a child process can't `cd` the caller or exec a zsh alias/function), so the
binary printed a small `key=value` **plan** to stdout and a `brain` zsh wrapper
executed it. That whole mechanism is **gone**. Brain now has a persistent TUI
and short-lived command families. Interactive file-open, Finder-reveal, PDF,
trash, and agent-launch actions live inside the persistent shell; focused
commands execute directly and exit. Nothing needs to mutate the parent shell,
so there is no plan protocol and no wrapper: `run.sh` just builds the binary
when the sources change and `exec`s it directly. The
agent launch that once relied on shell aliases is now driven by the
configurable `claude_cmd` / `codex_cmd` env values. See
[integrations.md](integrations.md).

## Why the TUI renders to `/dev/tty`, not stdout

The intentional stdout families are `config/env/version`, `workspace list`,
explicit plain-task output, and help. `--verbose` mirrors logs to stdout for
non-TUI commands. Clap errors and diagnostics go to stderr. The TUI renders to
`/dev/tty`, keeping full-screen escape codes and frame data out of stdout while
the interactive UI still reaches the real terminal. crossterm's
raw-mode toggles and event reader operate on the controlling terminal
regardless, so input is unaffected.

## Why we push the kitty protocol unconditionally (and avoid the probe)

Distinguishing `Ctrl-Enter` (reveal in Finder) from `Enter` (open file)
needs the terminal keyboard-enhancement protocol. We push
`DISAMBIGUATE_ESCAPE_CODES` on entry without checking support first:
unsupported terminals ignore the escape, and the matching pop is then a
no-op, so nothing is left in a bad state. We deliberately avoid
`supports_keyboard_enhancement()` because its `DA1 + CSI ? u` probe can
race teardown and leak bytes (`[?0u...[?...c`) into the parent shell on
slower terminals. The degradation is safe: without the protocol,
`Ctrl-Enter` just behaves like `Enter`.

## Why slug separators are stripped before fuzzy matching

Brain slugs look like `ann-afloat`, `2024_q3_review`, `rust.borrow`. With
nucleo's substring atoms, a query word like `afloat` wouldn't match
`ann-afloat` because the dash breaks the contiguous run. We normalize each
display string by dropping `-`, `_`, `.` and match against that, then map
the highlight indices back to the original bytes. Net effect: `afloat`,
`annafloat`, and `ann afloat` all find `ann-afloat`, and the highlight
still lands on the right characters. See [data-model.md](data-model.md).

## Why registry transactions use an adjacent SQLite lock database

Atomic rename prevents a torn `env.json`, but it cannot prevent two processes
from loading the same snapshot and replacing each other's successful changes.
Every registry writer therefore enters one `RegistryStore` transaction before
loading. The transaction holds `BEGIN IMMEDIATE` on a stable adjacent SQLite
database through mutation, validation, persistence, and create failure
reporting. The database has no schema or data and remains zero-length. With
`journal_mode=OFF`, `BEGIN IMMEDIATE` is an OS-backed lock that needs neither a
persistent initialization write nor an auxiliary journal file.

The stable database is deliberately never deleted. SQLite releases its OS lock
when the guard connection closes or its process exits, so a crashed writer
cannot leave a stale lock and a guard cannot unlink a replacement owner's
path. A stable PID sidecar supports typed timeout diagnostics and is likewise
never removed. This reuses Brain's existing direct `rusqlite` dependency,
preserves the Rust 1.85 MSRV, and avoids a `create_new` PID-file protocol whose
stale-file reaping has unavoidable path replacement races.

## Why failed workspace creation preserves every created directory

A create invocation can record each path for which its `create_dir` call
succeeded, but a later path-based identity check cannot prove that the same
object will still be present when `remove_dir` performs its own lookup. Another
actor can replace an empty directory between those two operations. Safe Rust
1.85 standard-library APIs do not couple that verification and deletion
atomically, so automatic cleanup could delete the other actor's replacement.

Brain therefore never deletes created directories after a later provisioning
or persistence failure. It retains the original failure as a structured source
and lists only paths created by that invocation, deepest first, so the user can
inspect and remove them manually. This leaves harmless empty directories in a
failure case in exchange for never deleting an ambiguously owned path.

## Why workspace readiness is one policy-driven bootstrap

Workspace selection cannot be an optional convenience around individual
handlers: a command that reads the wrong default before later honoring `-b`
has already crossed the silo boundary. Brain therefore classifies every parsed
invocation first. Help/version and hidden internal execution receive no
workspace and cannot prompt; create/attach/remove/repair receive only the
registry capability needed to establish or fix setup; every ordinary command
must receive one ready immutable `CommandContext`. There is no implicit fallback
policy for a new route.

The portable manifest is strict and separate from the machine registry. Its
workspace UUID proves that two machines mean the same workspace even when their
canonical names or roots differ; its receiver ingress UUID is portable; its
minimum version prevents an older binary from silently misreading newer state.
Unknown fields and schemas fail instead of being ignored because silently
accepting a changed identity contract is more dangerous than an actionable
upgrade error.

Create writes the manifest before registry persistence and deliberately leaves
`local_user_id` empty. This keeps create usable as a registry-only setup command
and makes the next ordinary command the single interactive onboarding point.
That flow creates the first portable person and selects it locally. Headless
callers receive exact `brain user add` and `brain user local` commands instead
of any `/dev/tty` access. On a later persistence failure,
Brain preserves the newly written manifest rather than performing a racy
path-based deletion; the matching identity remains inspectable and repairable.

An existing workspace with no portable user file and a non-empty legacy local
ID remains ready. Brain includes a pure legacy-conversion proposal so mappings
can be reviewed and tested, but bootstrap deliberately does not call it. This
compatibility gate avoids silently assigning old allowlisted contacts to an
invented person or changing a workspace merely because a newer binary opened
it.

Selection is also a capability boundary, not a convenience lookup. Root-local
stores accept `WorkspaceContext`; machine-env writes additionally require the
exact registry store and revalidate canonical name plus UUID. The TUI and its
background threads clone the same `Arc<WorkspaceContext>`. Detached Brain
children repeat the canonical `--brain` selector, and Brain-owned integrations
receive exactly workspace ID, canonical name, root, and actor ID. Therefore a
later default change cannot redirect an already-started operation.

## Why feature requirements do not replace workspace readiness

Readiness answers one blocking question: can this invocation safely bind a
root, portable identity, and local actor to the selected workspace? Optional
feature health answers a different operational question: is a deliberately
enabled integration usable? Folding both into one state machine would either
block unrelated commands on optional integrations or let malformed enabled
features masquerade as disabled.

Brain therefore keeps required availability distinct from optional `off`,
`ready`, and `incomplete` state. Startup reuses only the centralized required
field decision and preserves its existing repair behavior. Read-only status
surfaces inspect the already-pinned selected record and render redacted health;
they do not initialize defaults, recover transactions, create SQLite files,
render skills, or borrow setup from another workspace. Machine-local secrets
and sender addresses influence presence checks but never enter the requirement
model or formatter. The PDF row is informational here, while the existing TUI
startup PDF prerequisite remains a separate hard gate.

## Why changing the default never changes workspace policy

The machine default is routing metadata, not workspace data. It names the
canonical record selected when `--brain/-b` is absent. Keeping that field at
the registry top level means `set_default` can change one name without
rewriting either workspace record or portable files. Consequently, changing
the default workspace never changes access mode, UUID, root, local user,
receiver switch, aliases, or env.

Access mode stays portable because it describes the workspace's intended agent
behavior, while the machine default remains routing metadata. The first
migrated or created workspace is unrestricted for compatibility; later created
or attached workspaces start workspace-only. Existing valid portable values are
preserved. Valid-v2 startup validates only the selected root and seeds a missing
value from its current default/nondefault status before readiness succeeds.
Commands for a nondefault workspace therefore never mutate the default root;
whole-registry list and explicit migration are the only all-record checks.

Access-mode persistence is strict and atomic because a malformed portable file
must never be converted into an apparently valid unrestricted configuration.
Brain parses the existing JSON object before mutation, preserves unrelated
keys, syncs a same-directory temporary, atomically replaces the live file, and
syncs the parent directory. An interruption before replacement leaves the live
bytes untouched and cleans the temporary so a retry is safe.

Workspace-only uses the strongest trusted instruction surface each supported
frontend exposes, selected-root cwd, and a minimal child environment. It is
easy to bypass and serves only to reduce accidents and naive leakage among
trusted users. Adversarial users and sensitive isolation require an external
OS, VM, machine, or container boundary.
The naive literal-path classifier is only defense in depth: paraphrasing can
bypass it, so it must never grow into a claimed prompt-injection detector.

Clearing the inherited environment is insufficient if the PTY starts an
interactive or login shell: a profile can recreate a secret that filtering
removed. Agent commands therefore run through fixed `/bin/sh -c`. This keeps
the configured command string's ordinary shell parsing while deliberately
excluding profile, alias, and shell-function startup behavior. Initial prompts
are appended only after a standalone `--`, which prevents option-looking user
or inbound text from becoming a frontend flag or configuration override.

Ordinary command bootstrap now pins one immutable local actor before task,
reindex, TUI, or local-agent work. A ready legacy workspace with no portable
user store uses its exact lower-case kebab local ID as a non-writing
compatibility actor. Readiness rejects malformed nonblank legacy IDs with a
machine-local repair command, matching the actor parser exactly.
Inbound actor precedence remains immutable request context after provider
authentication. Task `assigned_to` now defaults to that actor, while unrelated
mutations preserve the existing assignment and explicit changes validate
portable membership. This deliberately adds no owner, creator, audit, or device
semantics. The agent-controller facade and fail-fast OpenCode selection stub
are active; shared receiver leases and authenticated forwarding are active as
well. Functional OpenCode behavior remains a later phase. Actor context is
attribution and routing, not a new authentication or access-control boundary.

The same portable user ID may be selected on multiple computers because it
names the person, not their machine. We intentionally add no cross-machine
identity split, owner, creator, or audit-history concept.

## Why triage enable/disable is one durable grouped replacement

The setting changes both policy and portable data. Saving JSON first could
leave disabled managed history behind; deleting CSV rows first could lose the
chains while config still claims they are enabled. Brain therefore stages the
config, task/habit CSVs, counter, and exact derived-reference rewrites, then
publishes a recovery journal before replacing any live file. Ordinary failures
roll back immediately. A later startup, reindex, config change, or repair first
recovers an interrupted prepared generation.

Stable `system_key` values, not visible names, decide ownership. This preserves
same-named user habits and permits user renames without losing protection.
Garbage collection retains its ordinary seven-day behavior while enabled and
never becomes an incomplete feature-off purge.

## Why assignment defaults to the effective actor

The request actor is already immutable and portable, so it is the only default
that behaves consistently for local and authenticated inbound work. User count
changes presentation, not semantics: one-person workspaces hide redundant
assignment controls but still persist the ID; shared workspaces reveal detail,
creation/reassignment controls, and filtering. Compatibility is intentionally
asymmetric: readers accept the legacy `assignee` heading, while any writer
normalizes to `assigned_to`. When both columns are present, `assigned_to` is
the canonical mapping input; falling back to whichever header appears first
would let stale legacy values override current portable assignment.

## Why task UUID migration runs only through the coordinated rollout

Human-facing `T###` and `H###` values are useful locators but cannot safely be
the permanent merge identity once two machines can allocate the same display
ID. New rows therefore receive immutable UUIDv4 identity, and migrated rows use
a deterministic UUIDv5 input scoped by workspace, CSV kind, and legacy display
ID. Completion and ordinary edits preserve the UUID; habit recurrence creates
a new UUID while retaining assignment and `system_key`.

Older task writers can leave one UUID on multiple existing rows. The rollout
repairs this before current-schema publication instead of failing a recoverable
migration. The first row in deterministic tasks-then-habits order retains the
existing UUID; each later duplicate gets a UUIDv5 derived from workspace, CSV
kind, original UUID, display ID, and row position. The repair is deterministic
and idempotent. If a journal resumes after remote schema publication,
verification repeats the repair locally and republishes current CSVs and
baselines under the migration lock before final verification, allowing the
remote copy to converge too.

Activation is deliberately separate. The schema helper requires an explicit
last-legacy-sync state, an existing durable machine-local backup base, and a
destination beneath that base; only `brain workspace migrate` calls it. Existing
legacy CSVs keep `task_id` identity so their semantic merge remains compatible;
schema-v2 CSVs merge by UUID, but only the explicit rollout coordinator may
invoke the activation helper. The UUID column alone is not activation:
compatibility writers may add `task_uuid` for
new rows while legacy rows remain blank, and sync continues to use `task_id`
until `tasks/SCHEMA.json` declares the coordinated current schema. The helper
rejects canonical or lexical path overlap with the workspace, creates each
missing backup-directory component separately, syncs
every actual parent on both first attempt and retry, durably syncs each exact
backup before replacement, and publishes an internal prepared/committed
recovery journal. A publication error removes the journal temporary before
returning. A retry restores
the complete legacy generation after a prepared interruption or retains the
complete new generation after commit, so a mixed schema is never accepted as a
new migration input. The coordinated rollout owns the final legacy sync,
all-machines and remote identity gates, sender mapping, backup activation,
rollout journal, derived rebuild, and final cross-store verification.

The final legacy sync can pull config, portable users, and assignments that did
not exist at command start. Brain therefore reloads all three immediately after
that journaled sync and before backup or portable mutation. Freezing pre-sync
objects would allow a newly pulled sender mapping to evade preflight or a stale
triage flag to be applied during resume.

A second configured legacy machine may begin migration after the first machine
has already published task schema v2. Treating the present remote marker as
legacy would publish incompatible rows, while ordinary schema matching would
strand the second machine. The coordinator therefore owns a replayable join
bridge before local activation: it runs generic rclone work, validates the
present remote manifest and both current CSVs, merges by the still-shared
`task_id`, preserves remote UUID authority for matching rows, and writes only
the local legacy generation. The following journaled cutover assigns
deterministic UUIDs only to local-only rows and retains schema-last publication.

Only actual absence or the recognized pre-v2 task-schema marker means legacy.
The compatibility marker must contain both `tasks_csv` and `habits_csv`
sections, each keyed by `task_id` and carrying a column list. Any other
present object is an authoritative protocol claim, so malformed JSON, missing
or wrong-typed required fields, unsupported versions, and a non-UUID merge key
must fail before task CSV reads or writes.

Task-schema activation is a distributed publication boundary, not only a local
file replacement. The rollout holds the UUID sync lock across local migration,
remote task and habit CSV publication, and durable local baseline creation.
Only after those four artifacts are ready does it publish
`tasks/SCHEMA.json`. Generic rclone sync excludes the two CSVs and schema
metadata, so no ordinary lane can reorder that transition. The rollout journal
blocks ordinary sync and setup until an interrupted transition resumes; this
closes the process-crash gap after the lock owner exits.

Recovery instructions are resume-only for every active rollout journal. A
remote copy can succeed before the matching journal record is durable, so even
a nominally pre-transition journal cannot safely authorize restoring only the
local machine. Retained backups support forensic inspection or a separately
coordinated manual recovery.

Backup publication treats the destination path as hostile after initial
validation. The verified temporary is created in the machine temporary
directory, then the destination parent is opened with `O_NOFOLLOW` and the
rename is performed relative to that open descriptor. This closes the
post-validation parent-replacement race without following a newly inserted
symlink, while preserving atomic publication and parent-directory syncing.

## Why both `tasks.csv` work and brain notes route through `brain`

Task management is a big domain with its own CSVs, recurrence rules, and TUI.
Since the tasks↔brain merge it is an **in-process main view** of `brain`
(`crate::tasks`), not a separate binary: `brain` reads the CSVs directly, and
`Ctrl-T` (or `brain tasks`) switches to the tasks view instantly with the brain
panel still beside it. The tasks logic stays in its own `src/tasks/` namespace
so it keeps its shape, but there is no cross-binary handoff (and no `tasks=1`
directive) anymore.

## Why the pure/impure split (and the `lib.rs`)

Every decision worth testing is pulled into a pure function:
`parse_config_root`, `expand_tilde_with_home`, `is_textlike`,
`finder_target`, `handle_key`, the `App` matching/navigation, the render
helpers, and `session::build_llm_command`. The thin shells that touch
`/dev/tty`, `$HOME`, the exe path, or `std::process::Command` stay
untested by design. `lib.rs` re-exports the modules so integration tests and the
thin binary entry point share one module graph. This avoids compiling a second
private copy of every module and keeps `main.rs` limited to bootstrap and
dispatch.

## Why bare `brain` is a persistent shell with a live agent panel

The agent session should stay available while the user works, not launch per
question and disappear. Bare `brain` is therefore a persistent shell with
three main views beside a live frontend-neutral PTY (the "brain panel"), rooted
in the selected workspace. Finding a note and thinking with an agent are
complementary, not modal. Startup focuses the tasks view while leaving the
panel open; `Alt+H` / `Alt+L` switch focus spatially and follow a layout swap.

## Why claude exiting closes the panel instead of quitting the shell

Exiting claude (Ctrl-C, Ctrl-C) is a frequent gesture: you end a chat
without meaning to leave `brain`. So when the `claude` child dies the event
loop **closes the panel** (explicitly shuts down its controller, search goes
full-width) rather than quitting; the closing Ctrl-C is forwarded to claude
and never seen as a quit,
and the auto-close needs no extra keystroke. Quitting `brain` is a separate,
deliberate gesture: `Esc` / `Ctrl-c` from the **search** panel. Re-opening is
**Message brain** (`Ctrl-M` or the palette), which resumes your latest
session — so the panel is closeable and re-openable, not a one-shot.

Closing the panel **releases** the session lock (it's no longer being driven)
so the re-open goes through the same recency+claim path as startup — which is
also why "Message brain" appears in the palette only while the panel is
closed: there's nothing to open when it's already up (and `Alt+L` focuses it).

## Why opening a file spawns a new iTerm2 tab instead of replacing the shell

In the persistent shell the whole point is that the brain panel never goes
away. Running `$EDITOR` *in the current terminal* would tear down the TUI to
edit a note. Instead the running TUI spawns a **new iTerm2 tab** (`osascript`)
with `cd <dir> && $EDITOR <file>` for text files, and `open <file>` for
everything else (which launches its own app).
Either way the brain shell stays up. iTerm2 is the user's terminal, so we
drive it directly; on any other terminal the editor path falls back to
`open`.

## Why SQLite (not a JSON file) for session + layout state

The state is written by *multiple* processes that can race: several `brain`
shells, plus the Claude SessionStart hook (a separate Python process) firing
on every session start/resume/`/clear`. A JSON file would need hand-rolled
locking to stay consistent; SQLite in WAL mode gives concurrent readers + a
single writer with no busy-storms for free, and the `tasks` sibling already
established the pattern. The cost is `rusqlite` (`bundled`), accepted for the
concurrency guarantee.

## Why the lock + recency resume model (the multi-terminal answer)

Two goals tension: *always resume your latest conversation*, but *never put
two terminals on the same thread* (which would interleave into a tangle).
The resolution: each running shell **locks** its session to its PID; on
startup a shell resumes the most-recently-active **free** session (or starts
fresh if none is free) and releases the lock on exit. One terminal always
resumes its last conversation; a second can't grab the one the first holds,
so it takes the next-free session or a fresh one. Crashes don't strand a
session — dead-PID locks are reaped (`kill -0`) on the next startup.

## Why SessionStart and Stop hooks have distinct jobs

brain can choose a session id up front (`--session-id`), but if the user
types `/new` (or `/clear`) mid-run, Claude may rotate to an id brain never
saw. That fresh conversation is the one they would want to resume next time.
A **SessionStart** hook fires on every start/resume/clear/compact with the live
`session_id` (keyed to the shell via `BRAIN_INSTANCE_ID` / `BRAIN_PID` env),
so brain always learns the current id and returns the exact scoped row to
`active`.

The **Stop** hook has a separate, per-turn responsibility. It writes the
authenticated completion artifact and marks that same scoped row `completed`,
which lets queued receiver work advance. It does not end the persistent
conversation or make the PTY disposable. The next successful local or queued
submit calls `SessionStore::mark_active`, so ordinary turns after the first one
do not depend on another SessionStart event to reactivate the row.

Stop authorization, artifact publication, and completion mutation form one
ordered operation. The hook stages a unique synced response file, acquires
`BEGIN IMMEDIATE`, and rechecks the exact currently locked frontend, session,
workspace, actor, channel, and Brain-instance tuple. Its update uses that same
predicate and must affect exactly one row. The response artifact is atomically
published and its directory synced before the database commits `completed`.
If publication or commit fails, the transaction rolls back and the hook
removes or restores only its own published inode. This ordering forbids a
committed completion without its artifact. It also makes a concurrent
SessionStart rotation win or serialize before Stop rechecks the old lineage,
instead of allowing a stale parsed payload to complete an unlocked row.

Rotation authorization and mutation must be one write transaction. A target
ownership `SELECT` followed by a later unconditional upsert leaves a race in
which two shells can both authorize the same free target and the last writer
can overwrite the first. The hook therefore acquires `BEGIN IMMEDIATE` before
reading the exact tuple, source lineage, or target owner. Contenders wait at
the transaction boundary, then re-read current ownership; authorization
no-ops and exceptions explicitly roll back, while the target upsert and prior
session release commit together.

## Why the Stop hook reads the transcript, not just `last_assistant_message`

The final response the user receives over SMS/email exists only if the Stop
hook writes the response artifact, and there is no hook-independent backstop
for a still-alive panel: `App::close_brain`'s PTY-scrape fallback runs only
after the agent process exits, which never happens for brain's persistent
Claude session. So the hook is the single trigger, and it must not depend on a
single optional field. `last_assistant_message` is a Claude Code convenience
field: whenever a frontend build, mode, or turn shape omits it (a turn that
ends on a tool call with no trailing text, a schema change, an older/other
build), keying on it alone makes the hook a silent no-op and the user gets only
the two-minute "still processing" notice with no final answer. The hook
therefore resolves the final message defensively — prefer
`last_assistant_message` when present and non-empty, else parse the last
assistant text message from the Stop payload's `transcript_path` JSONL
(`{"type":"assistant","message":{"role":"assistant","content":[{"type":"text",…}]}}`),
joining that message's text blocks. This is forward- and backward-compatible
across Claude Code versions, keeps the Rust consumer unchanged (it still reads
`{session_id, message, …}`), and leaves a genuinely empty turn as the only
no-op. A regression test exercises a realistic Stop payload with no
`last_assistant_message` but a transcript present.

## Why we verify a transcript exists before resuming a session

`claude --resume <id>` only works if a transcript `<id>.jsonl` exists in the
project dir — and Claude writes that file only once a message is exchanged.
So a brain session you open and close *without chatting* leaves a DB row
with no transcript; blindly `--resume`-ing it later produces "couldn't find
session with ID …". brain therefore checks the transcript exists on disk
before resuming and skips candidates that don't, falling back to the next
valid one (or a fresh chat). This is also why brain forces the PTY's cwd to
`<brain_root>` before the child starts: every session is scoped to the same
project dir, so the existence check and `--resume` always
look in the same place. When the fallback to a fresh chat is caused by a
missing transcript, we surface it in the status line rather than silently —
the user asked to know when their conversation didn't carry over.

## Why the brain panel launch is frontend-aware

The TUI and receiver call the `AgentController` facade's semantic launch,
input, lifecycle, completion, terminal, and shutdown operations. Only the
Claude and Codex adapters translate those requests. The inert OpenCode adapter
implements the same shape but returns `UnsupportedFrontend` before transport
access for every operation.

The brain panel must control frontend-specific session arguments, so it can't
defer to a shell alias that might inject incompatible flags. Claude remains the
default and uses `claude_cmd` in brain env (default
`claude --dangerously-skip-permissions`); brain appends its own `--resume` /
`--session-id` after that configured base command. A legacy
`brain config claude_cmd` value is honored only when env has no `claude_cmd`,
so existing installs keep working while new edits are machine-local.

Codex is selected per run with `--codex` / `-cx` and uses `codex_cmd` in brain env
(default `codex`) because the right Codex wrapper/model flags can be
machine-specific. `session::build_llm_command` splices either configured base
command in verbatim, then appends the selected frontend's own session shape:
Claude gets `--resume <id>` or `--session-id <id>`; Codex gets `resume <id>`
only when a Codex id is known and no Claude-only flags for fresh launches.

The state DB keys sessions by frontend, opaque session ID, workspace, actor,
and channel. Hook upserts, claims, and dead-lock reaping use that exact
composite scope, so equal opaque IDs in different scopes never overwrite or
unlock one another. A separate stable response ID lets the Stop hook signal a
fresh Codex turn without pretending Brain chose Codex's thread ID.

Main-panel launch completes fallible capability resolution and adapter response
identity lookup before claiming a resumable row. Once claimed, only request
assembly and the guarded controller launch remain; a launch failure releases
the instance claim. This keeps a malformed capability configuration or
frontend identity error from removing an otherwise free conversation from the
resume queue, and every failed path clears the response identity for the launch
slot it attempted.

Hook refresh follows workspace singleton acquisition, so a rejected second TUI
cannot alter the lifecycle contract of the live process. Different-workspace
TUIs remain concurrent, so their shared `~/.codex/hooks.json` updates use a
machine-wide SQLite transaction lock plus same-directory atomic replacement.
The lock prevents lost read-modify-write updates; the rename prevents readers
from observing partial JSON and preserves the old bytes on failure.

## Why workspace capabilities reuse shared frontend authentication

Workspace capability selection is configuration, not a second identity. Brain
therefore keeps portable logical allowlists in the root while commands, URLs,
paths, and credentials stay in that workspace's selected machine record. It
does not create Claude or Codex auth profiles.

Claude has a conditionally verified strict MCP boundary: a cache-local
generated JSON plus `--mcp-config --strict-mcp-config`, when Brain can safely
parse the configured command as a direct Claude invocation. Shell-indirect,
ambiguous, or conflicting commands remain supported but report advisory
enforcement because appended flags are not proof that Claude receives them.
`--bare` is intentionally excluded because it changes authentication behavior.
Claude's installed skill controls cannot
strictly select an arbitrary subset, so skill names remain advisory. Codex's
documented `-c` overrides merge with base config. Namespacing generated server
keys prevents collision with known global names, while per-server wrappers
prevent same-named stdio secrets from colliding with frontend environment.
Neither mechanism can prove that other global servers are excluded, so Codex
remains advisory. Enforcement status is derived from the concrete command and
launch flags and never upgraded from advisory by logical selection alone.

Capability selection is mandatory controller context in workspace-only mode,
not optional adapter decoration. The controller accepts exactly two context
shapes: unrestricted mode without a plan, or workspace-only mode with a plan
whose mode and credential provenance match the selected workspace. Every other
mode/plan combination is rejected before frontend or transport work.
Unrestricted launch construction deliberately skips capability parsing and
clears stale capability artifacts, so malformed unused configuration cannot
break or influence ordinary frontend behavior.

That skip begins at TUI setup, before `App` construction. Startup first parses
`access_mode` and every live non-capability setting strictly. It omits only the
unused logical capability lists when the validated mode is unrestricted;
workspace-only startup retains strict list parsing. This keeps malformed dead
configuration from blocking either frontend without weakening the mode gate.

Selected skill copies live under the workspace UUID and actor cache. They do
not link to, prune, or rewrite the shared registry. This preserves the user's
ordinary unrestricted environment and avoids one workspace launch changing
another workspace's frontend state.
Exact configured source loading and symlink rejection keep a logical name from
redirecting rendering to a sibling or later-retargeted tree. Runtime MCP files
and wrappers are short-lived frontend artifacts: frontend switches remove the
other frontend's files, retries remove abandoned temporary files, and durable
renames sync the containing directory.

For machine skill sources, the root-owned first absolute path component is the
trust anchor. Brain canonicalizes that anchor, proves the full source remains
below it, rejects every lower ancestor symlink, then retains the canonical
source path. This narrowly permits platform-owned aliases such as `/var` while
rejecting parent links controlled within the configured tree. For generated
artifacts, the selected UUID cache directory is the trust anchor instead;
recursive removal validates it and each target ancestor without following a
symlink, then fails closed on any mismatch.

## Why we disable alternate scroll (and motion) reporting for the mouse

`EnableMouseCapture` turns on more than we want. We immediately trim it with
a raw `\x1b[?1002l\x1b[?1003l\x1b[?1007l` (see
`tui::disable_mouse_motion_reporting`):

- `1002`/`1003` (button-drag + any-event **motion**) off so ⌘-hover / ⌘-click
  still reach iTerm2's native link / Semantic-History handler; we only need
  button + wheel events.
- `1007` (xterm **alternate scroll**) off because it is the subtle reason the
  wheel appeared dead in the brain shell. On the alternate screen, iTerm2
  (default) and xterm translate the wheel into **arrow keys** instead of mouse
  events. The brain shell opens with the brain panel unfocused, so those
  arrows were forwarded straight into `claude` and *neither* panel scrolled.
  Disabling alternate scroll forces real wheel mouse events, which
  `handle_mouse` routes to whichever panel the cursor is over, independent of
  which panel has keyboard focus. (The `tasks` sibling only seemed unaffected
  because it opens focused on its list panel, where stray arrow keys move the
  selection and look like scrolling.)

Both are best-effort DECRST writes: a terminal that doesn't speak them just
ignores the escape, so there's no teardown to undo.

### Update: the wheel is unreliable in practice, so we lean on `Alt+U`/`Alt+D`

The reasoning above assumed that after trimming to **button + wheel** (plain
`1000` reporting, motion off), iTerm2 would still deliver wheel events to the
app. In practice that assumption does **not** hold on the user's iTerm2: with
motion reporting off, the scroll wheel produces *no* mouse events at all, so
neither panel scrolls with the wheel. The two mouse concerns are effectively
in tension on this terminal:

- **Wheel to the app** appears to require `1002`/`1003` (motion) reporting to
  be *on* — plain `1000` alone does not emit wheel events here.
- **`1002`/`1003` off** is what the original decision chose so ⌘-hover /
  ⌘-click keep reaching iTerm2's native Semantic-History handler.

We deliberately did **not** re-enable motion reporting to chase the wheel:

1. We can't confirm (without an interactive terminal probe) that ⌘-click
   survives motion reporting on the user's exact iTerm2 version, and silently
   breaking ⌘-click would be worse than a dead wheel.
2. Any escape-sequence answer is terminal-specific and fragile; these shells
   should not depend on a particular terminal's wheel semantics.

Instead, **`Alt+U` / `Alt+D` are the supported way to scroll** (half-page up /
down of the focused panel; see [keybindings.md](keybindings.md)). They are
handled as ordinary key events, intercepted before forwarding to the selected
agent, so they work in every terminal, in every panel, even while Claude or
Codex has focus or the search filter is being typed, with **zero** dependency
on mouse reporting. Brain also accepts the macOS Option-produced `U` and `D`
glyphs as equivalent scroll chords, because richer keyboard modes in embedded
frontends can surface those glyphs instead of Alt-modified ASCII. The `1007`
(alternate-scroll) DECRST is still worth keeping: it is cheap, harmless where
unsupported, and keeps the wheel from turning into stray arrow keys for
terminals where the wheel *does* reach the app. The `tasks` and `dif` siblings
make the same call and document it inline in their
`keybindings.md` mouse sections (neither keeps a decisions log).

## Why "Create PDF" is `Ctrl-G` and a contextual, leading palette row

The action only makes sense for a markdown file, so both its palette row and
its shortcut are **gated on a `.md` selection** — the row is absent otherwise,
and `Ctrl-G` is a no-op. It's `Ctrl-G` ("generate"), *not* `Ctrl-M`: `Ctrl-M`
is already "Message brain" (and shares Enter's byte), and shadowing it on the
common case of a highlighted note would be worse than a free letter. The row
**leads** the palette (before "Message brain") when present so opening the
palette on a markdown file lands on it by default — the fast path is
`Ctrl-p` → `Enter`. The confirmation modal fires **only** from the `Ctrl-G`
shortcut, not the palette row: picking a row is already a deliberate
confirmation, whereas a bare keystroke is easy to hit by accident.

## Why we delete any existing PDF before converting

The user's `markdown-to-pdf` never overwrites in non-interactive mode — it
writes a `-vN` variant instead (a safety default for scripted callers). But
"Create PDF" promises a PDF named exactly like the source (`plan.md` →
`plan.pdf`), and we open that exact path afterward. So `create_pdf` removes any
pre-existing same-name PDF first, guaranteeing the converter writes the exact
name and that we open the file we just produced. This is a regenerate action on
a derived artifact, so replacing the previous output is the expected behavior,
not data loss.

## Why "Delete" trashes (not `rm`), defaults to No, and trails the palette

Delete is the one destructive action `brain` performs, so every choice around
it leans safe:

- **Trash, not `rm`.** `move_to_trash` asks Finder to `delete` the item, which
  lands it in the Trash exactly like a user dragging it there — recoverable via
  `Put Back`. A raw `rm` would make a fat-finger unrecoverable, which is the
  wrong default for a note system. We reuse the OS Trash rather than inventing
  our own "deleted" area (mirrors how "Create PDF" reuses the user's converter).
- **Red modal, default No.** Both the `Ctrl-D` shortcut *and* the palette row
  route through the confirmation modal (unlike "Create PDF", whose palette row
  skips it) — there is no non-confirmed path to a delete. The modal is red and
  defaults to **No**, so a stray `Enter` cancels; deleting takes a deliberate
  `y` or a toggle to Yes. (Contrast PDF, a constructive action, which is green
  and defaults to Yes.)
- **Trails the palette.** The "Delete '…'" row is appended **last**, never the
  default-selected top row, so `Ctrl-p` → `Enter` can't delete by reflex. The
  "Create PDF" row, being safe and the likely intent on a note, leads instead.
- **`Ctrl-D`, search-panel only.** Bound in the search panel's key handler, not
  intercepted globally, so it never sends EOF (`Ctrl-D`) to `claude` when the
  brain panel is focused. `Ctrl-R` (refresh) is scoped the same way to avoid
  claude's reverse-search.

## Why the search list auto-refreshes (and `Ctrl-R` exists)

The picker walks the tree once at open. Actions that change the tree from
inside `brain` — creating a PDF, deleting an entry — would otherwise leave a
stale list until the next scope switch. So those actions call `App::refresh`
(re-walk the current `scope`, keep the query via `reload_entries`), and
`Ctrl-R` exposes the same refresh manually for changes made outside `brain`
(a file added in another terminal, an editor save). Refresh keeps the query
where a scope switch (`set_entries`) clears it, because a refresh is "same
view, newer data" rather than "new view".

## Why `markdown-to-pdf` is a discovered, configurable command

`markdown-to-pdf` is often installed as a shell **function** (an autoloaded
wrapper), which a child process can't invoke, so the binary needs a concrete
executable to spawn. Rather than shell out to an interactive `zsh -ic` on every
conversion (slow, sources the whole rc, risks leaking output onto our
`/dev/tty`), `brain` stores the tool's path as an env variable
(`markdown_to_pdf_path`, in brain env — see "Why brain env split off from
brain config" below) and spawns it directly with `<file.md> --out`.

The path is not hardcoded (the repo is public). On first run it is
**auto-discovered** (PATH, then conventional bin dirs, then a one-shot login
shell that resolves an autoloaded function to the script it wraps) and
persisted. A missing or invalid path is a hard, fail-fast error pointing at
`brain env set markdown_to_pdf_path=…`. See `settings/markdown_pdf.rs`.

## Why `linear_workspace` is a slug, not a full URL

The Linear link config is the **workspace slug** (e.g. `acme`), not the whole
`https://linear.app/<slug>/issue/` prefix. The slug is the only part that
varies per user; brain owns the URL shape, so a user configures the minimum and
can't get the surrounding format wrong. `Config::linear_base_url` interpolates
it, and an empty slug simply omits Linear links.

## Why portable config lives inside the brain root (`<brain-root>/.config/`)

Everything Brain persists as portable config, including `config.json`,
`personalization.json`, skill `extensions/`, and `plugins/`, lives in
`<brain-root>/.config/`. The design went
through three positions and this is the resting one:

1. **A/original:** machine-local `config.json` at `~/.config/brain`, but
   personalization *inside* the brain root so it synced with the brain.
2. **Mid-project:** unify *everything* under `~/.config/brain` and sync that dir
   externally (via a dotfiles manager's tracked home tree).
3. **Final (here):** unify everything *inside the brain root*.

Position 2 died on a concrete footgun. A common dotfiles-manager design syncs
`$HOME` paths by symlinking them at a **regenerated mirror** (a build directory
wiped and rebuilt on every sync). A tool that *writes its config at runtime*
(brain does: `config set`, `personalize set`, onboarding, `resync_skills`)
writing through such a symlink lands its bytes in the gitignored mirror, which
is then wiped on the next build — the write is lost and never committed. A
dotfiles-manager-native tool can dodge this by writing the repo *source*
directly, but brain can't: it must stay generic and **never know about any
particular dotfiles repo**.

So instead of routing brain's runtime writes through any dotfiles mirror, we put
its config *inside the brain*. The brain root is already the user's synced,
portable content; brain's own state rides along with it for free (whatever syncs
the brain — sub-project C, Backblaze, etc. — syncs the config), and **the
dotfiles manager has nothing to do with brain config at all**. The repo stays
generic (no tracked `config.json`), and brain still writes only under `$HOME`.

### Historical exception: the legacy root pointer (superseded)

Before schema v2, the brain-root location could not live inside that root, so
Brain read one path from `~/.config/brain-root` and otherwise used `~/brain`.
That pointer was the only machine-local state and was safe for a dotfiles tool
to mirror because Brain never wrote it.

Schema v2 supersedes that historical decision. The sole machine registry
at `$XDG_CONFIG_HOME/brain/env.json` (fallback
`~/.config/brain/env.json`) now owns one structural root per workspace. The old
pointer and `~/brain` remain read-only one-time migration inputs only. New
roots use `brain workspace create` or `brain workspace attach`; free-form
`brain env set` cannot write `root`.

### `markdown_to_pdf_path` and syncing a machine-specific value (historical; superseded)

At the time of this decision, `config.json` synced across machines and
`markdown_to_pdf_path` rode along inside it despite being machine-specific (the
binary sits at a different path per host); the startup gate compensated with a
self-heal (a stored path missing/not-executable on *this* machine triggers
re-discovery and re-persist before failing). **C1 (see "Why brain env split off
from brain config" below) removes the need for that compromise**:
`markdown_to_pdf_path` now lives in brain env, which never syncs, so there is
no synced-but-stale value to heal from in the first place. The startup gate
still re-discovers on an invalid path (a fresh machine that has never run
`brain` there yet has nothing to heal from), but the sync-races-a-machine-local-value
tension this subsection originally described no longer exists.

## Why brain env split off from brain config (C1) — and why it partially reverses "config lives inside the brain root"

Sub-project C (cross-machine sync of `~/brain` via Backblaze B2) makes the
brain directory itself sync across machines. That flips the risk the previous
decision was optimizing for: once the brain dir is what syncs, anything
machine-specific sitting inside `<brain-root>/.config/` — a discovered
`markdown_to_pdf_path`, and especially B2 credentials — would sync too, leaking
secrets and wrong-on-another-host paths onto every machine. So C1 splits
config into two stores by **lifecycle**, not just location:

- **brain env** (`~/.config/brain/env.json`, machine-local, `brain env` CLI) —
  anything that would be *wrong* if copied to another machine: `root`,
  `markdown_to_pdf_path`, `claude_cmd`, `codex_cmd`, and the Backblaze `sync`
  block (bucket, credentials, trigger flags). Lives at a fixed XDG-style path
  **outside** the brain root, so it is structurally exempt from whatever syncs
  the brain directory.
- **brain config** (`<brain-root>/.config/config.json`, `brain config` CLI) —
  everything that's *right* on every machine: `linear_workspace`,
  `daily_triage_name_pattern`, `day_rollover_hour`, `agenda_dir`,
  `calendar_id`, `skills_auto_sync`. It keeps riding the brain-dir sync.

**The rule of thumb:** *wrong-if-synced ⇒ brain env; right-on-every-machine ⇒
brain config.* Personalization (`personalization.json`, `extensions/`,
`plugins/`) is unaffected — it's content *about you*, correct everywhere, so it
stays a third store inside the brain root alongside `config.json`.

This is a **partial reversal** of "Why config lives inside the brain root"
(above), which deliberately unified *everything* — including the
then-machine-specific `markdown_to_pdf_path` — inside the brain root to dodge
the dotfiles-mirror write footgun. That reversal is now correct because C makes
the *opposite* failure mode dominant: a synced brain dir means anything
machine-local placed inside it leaks across every machine on the next sync,
whereas the dotfiles-mirror write footgun the original decision was avoiding is
merely inconvenient (a lost local edit) rather than a wrong path or a leaked
secret landing somewhere it shouldn't.

Splitting off brain env also gives the machine registry a non-circular home for
workspace roots. In schema v2, `root` is structural `WorkspaceRecord` data,
not writable free-form env. The legacy `~/.config/brain-root` pointer remains
a back-compat read and is folded into the first record during migration.

**The residual dotfiles-mirror write footgun, now on `env.json`.** `env.json` is
runtime-mutable (`brain env set`, and the `markdown_to_pdf_path` self-heal), so
the same footgun the original decision dodged for `config.json` now applies to
it: if a dotfiles manager mirror-*symlinks* `env.json` at a regenerated mirror,
a runtime write lands in the mirror and is lost on the next rebuild. Brain does
not solve this — it stays generic and has no dotfiles-manager awareness by
design. The fix, if a user wants `env.json` to persist across machines via a
private dotfiles repo, is **dotfiles-manager-side**: seed/copy the file into
place rather than symlinking it (or re-commit after changes), the way a
dotfiles manager can safely symlink the read-only `~/.config/brain-root`
pointer (safe because brain only ever reads it — `env.json` is not read-only,
so the same trick doesn't apply). See the brain-sync design spec §12 for the
full record.

## C2/§19 — `brain sync`'s rclone transport: env-var creds, `--max-delete` + `--check-access`, bisync over a custom merge

C1 shipped only a parse-only `SyncConfig`; C2 is the first phase that actually
moves bytes. The choices below are the ones a future phase (a triggers/watch
phase, or a conflict-resolve UI) needs to keep, not accidentally "simplify."

**Why rclone remains external instead of bundled.** Rclone is an independently
released cross-platform transport binary, while brain owns the user-facing
workflow and safety/bookkeeping around it. Bundling it would couple brain's
release, signing, and architecture updates to rclone's. The sync command now
checks for rclone before remote work and prints both the Homebrew and official
installer commands when it is absent, making the dependency explicit without
duplicating its distribution pipeline.

**Why credentials are `RCLONE_CONFIG_*` env vars, never a persisted
`rclone.conf`, and never on argv.** The brain-env `sync` block already holds
the B2 key id/app key as plaintext (machine-local, never synced — see the C1
decision above), so `brain sync` doesn't need a *second* secret store; it just
needs to hand rclone the same secret at invocation time. `src/sync/remote.rs`
builds an rclone remote (named `BRAIN`) entirely as env vars
(`RCLONE_CONFIG_BRAIN_TYPE`/`_ACCOUNT`/`_KEY`) passed to the child process,
with the argv carrying only `BRAIN:<bucket>/<path>` — no secret in it. Two
things this avoids: (1) a persisted `~/.config/rclone/rclone.conf` that would
be a second, harder-to-audit copy of the same credential, sitting outside
brain's own config lifecycle; (2) secrets on the process argv, which any other
user on the machine can read via `ps`. Rebuilding the remote from brain env on
every invocation costs nothing (it's a handful of string ops) and keeps the
credential's *only* copy where `brain env`/`brain sync setup` already manage
it.

**Why `rclone crypt` is an optional env-defined layer, not a new sync
surface.** C2 chose private-bucket server-side encryption first because it has
no extra passphrase to escrow or lose. The §19 crypt slice adds the zero-knowledge
option without changing `brain sync`: if `sync.crypt_password` is empty,
`build_remote` returns the existing `BRAIN:<bucket>/<path>` target; if it is
set, the same function defines `BRAINCRYPT` in env vars and returns
`BRAINCRYPT:`. That keeps the transfer, CSV merge, check-access marker, journal,
and trigger code oblivious to the encryption layer. The tradeoff remains
explicitly on the user: brain can store rclone-obscured values in machine-local
env, but it cannot recover encrypted remote data after the original passphrases
are lost.

**Why the portable manifest is a mandatory remote ownership gate.** A bucket
and path are location, not identity. Two selected workspace records can be
misconfigured to point at the same location, and the `RCLONE_TEST` marker only
proves path symmetry. Before any check-marker, bisync, CSV, counter, or portable
mutation, brain therefore compares the selected UUID with the strict remote
`.config/workspace.json` and exposes the remote to mutation code only through a
verified capability. Mismatch and invalid manifests fail closed. A missing
manifest is safe to initialize only when setup proves the remote has no files;
setup first publishes exact manifest bytes under an append-only UUID-named
claim and reads the claim back. A newly staged claim ends that attempt without
touching the canonical path. On retry, setup enumerates and validates all
durable claimants and elects the lowest UUID. Only the winner may publish the canonical manifest, and it
re-probes that path immediately before using immutable-copy defense. This claim
protocol is necessary because the rclone/B2 surface does not expose a portable
compare-and-swap for `.config/workspace.json`; distinct claim names avoid the
original shared last-writer-wins object race. Claims are excluded from ordinary
transfer and remain available for safe setup retry. The canonical bytes are
read back before persisting credentials or writing data. Existing local manifests are never
rewritten, and ordinary transfer excludes the manifest so it cannot replace a
remote owner's identity. A nonempty manifestless remote can be legacy data for
the selected workspace, but absence alone cannot prove ownership. Setup
therefore displays the local canonical name and UUID, configured target, and
observed remote status before requiring a positive interactive confirmation.
Automation must instead repeat the exact selected UUID through
`--adopt-workspace-id`; a generic `--yes` is intentionally insufficient. This
authority applies only to setup. Mismatched, untrusted, or present-but-unreadable
manifests and every ordinary or internal sync path remain hard refusals.
Authorized adoption still publishes and reads back the exact existing local
manifest before credentials, markers, CSVs, counters, or bisync data can be
written.

**Why `--max-delete` shipped first, and why `--check-access` is now enabled.** rclone offers `--check-access`
as a symmetry guard (both sides must show matching `RCLONE_TEST` marker files,
or the run aborts) and `--max-delete` as a blast-radius guard (abort if a run
would delete more than a configured percent of files). C2 shipped only
`--max-delete` because `--check-access` requires brain to create and manage
marker files on both sides; enabling it before that lifecycle existed would
make every run abort. The §19 hardening slice adds that lifecycle in
`src/sync/check_access.rs`: setup/repair write a generic local `RCLONE_TEST`
marker, copy it to the remote root with rclone `copyto`, and then run the
baseline resync. Normal syncs now pass `--check-access --check-filename
RCLONE_TEST`. A missing marker triggers the narrow automatic repair path on a
normal sync: brain recreates the local and remote marker and runs one resync.
An explicit `brain sync repair` remains available for deliberate recovery.

**Why `rclone bisync` and not a from-scratch merge.** The brain root isn't
just markdown notes — it also holds `tasks.csv`/`habits.csv`, which don't
merge line-by-line the way prose does. Rather than write and maintain a
CSV-aware three-way merge, C2 leans on `rclone bisync`, which already
implements correct bidirectional sync semantics: it tracks each side's prior
listing so it can distinguish "changed here" from "deleted there" (the
half of bidirectional sync that's easy to get wrong — naively diffing two
current snapshots can't tell an edit from a delete-then-recreate), and it
ships conflict handling and delete-propagation guards brain would otherwise
have to reinvent. A CSV-specific merge strategy, if ever needed, is a
narrower problem to solve later on top of this transport, not a reason to
avoid it now.

**Why a same-file conflict is "keep both," not "pick a winner and discard."**
`--conflict-resolve` (`newer` for a bare `brain sync`, `path1`/`path2` for
`--push`/`--pull`) decides which copy rclone treats as the winner for *this
run*, but `--conflict-loser pathname` (not the default `num`) tells it to keep
the loser too, under a suffix, rather than delete it. Silently discarding a
same-file edit on a personal knowledge base is worse than a little clutter —
the loser might be the copy the user actually wanted. brain's post-pass then
renames the marker to a human-readable `name (conflict <host> <date>).ext`
(see [data-model.md](data-model.md)) so resolving it later is a normal
file-manager task, not spelunking for `__brainconflict__` files.

**Why there is no silent auto-resync (partially superseded — see C3 below).**
When bisync aborts (the `--max-delete` guard, or rclone's own "prior listings
missing" guard), C2 deliberately does not retry with `--resync` on its own —
`--resync` makes one side unconditionally overwrite the other's listing, and
blindly doing that after an abort could paper over exactly the kind of change
(a wiped directory, a botched previous run) the guard exists to catch.
Instead the abort is surfaced (`Outcome::Aborted`, with a message pointing at
`brain sync repair`) and the human decides, except for the narrow
`PriorListingMissing` resync and missing check-access-marker repair paths
described below. This mirrors the project's broader pattern of surfacing
rather than auto-healing anything that touches data loss
(contrast the auto-healed
`markdown_to_pdf_path`, which only ever *rediscovers a tool path* — never
anyone's data). **This still holds for `--max-delete`** — a tripped delete
guard always surfaces for a human decision, never auto-retried. C3 narrows
the "no auto-resync" rule to that one case; see below for why
`PriorListingMissing` specifically is safe to auto-retry.

## Brain sync progress, resume, and selective sync (built on C2, pre-C3)

Built on top of C2 after a real first sync on a 144 GB brain surfaced three
gaps: a long baseline looked like a silent hang, an interrupted sync had no
easy resume path, and there was no way to keep giant non-note files out of
the bucket. (This is the interstitial progress/resume work, not phase C3 —
C3 is the id-keyed CSV semantic merge below.) See the design spec
(`docs/superpowers/specs/2026-07-25-brain-sync-progress-resume.md`) for the
full write-up; the durable decisions are below.

**Why stream rclone's output instead of capturing it.** `Command::output()`
(C2's original approach) buffers everything and blocks until exit — on a
multi-hour first baseline the user sees nothing until it's done, which is
indistinguishable from a hang. `src/sync/run.rs` now inherits stdout for the
child and pipes only stderr (rclone's log/stats stream), reading it
line-by-line on the same thread that spawned the process: each line is
echoed live *and* appended to a capture buffer used for the post-exit parse.
One pipe, drained continuously — no second thread, no deadlock. Paired with
`--stats 10s --stats-one-line` in the bisync argv, this turns a silent block
into a periodic one-line progress readout (files/bytes/%/rate/ETA).

**Why `PriorListingMissing` gets an automatic one-shot resync but
`MaxDelete` still does not.** These are rclone's two abort kinds, and they
carry very different risk profiles. `MaxDelete` means bisync is *about* to
delete more files than the configured guard allows — auto-retrying that
could propagate a real, intentional-looking mass delete without a human ever
seeing it, which is exactly the harm the C2 decision above was written to
prevent. `PriorListingMissing` means the opposite: bisync's own baseline
bookkeeping is incomplete, almost always because a previous `--resync` was
killed mid-run (Ctrl-C, a crash, a dropped connection) before it could
finish writing both sides' listings. Resuming that with another `--resync`
doesn't discard or override anyone's data — it re-establishes the baseline
using whatever state already exists on both sides and uploads only what's
missing, which is the intended recovery path in any case (the *manual* fix
was already "run `brain sync repair`" under C2). Automating just this one,
narrow, low-risk case is what `command::should_auto_resync` encodes (pure:
`dir != Resync && abort == PriorListingMissing`, so it fires once and never
loops on a resync's own abort), paired with `--resilient --recover` in the
bisync argv so rclone itself tolerates a transient interruption without even
reaching the abort path.

**Why brain never journals `clean` for an interrupted or errored run
(the never-miss guarantee).** Auto-resume is only safe if brain never lies
about a run's completeness first. `verify::classify` already only returned
`Clean` on a zero-error, fully-successful rclone exit; C3 leans on that same
invariant as the backbone of the "no file left un-synced" guarantee — an
interrupted run is always `NeedsAttention`/`Aborted`, so either the
auto-resume above or the next plain `brain sync` picks the job back up,
rather than a user trusting a false "done" and never running sync again.

**Why deletions propagating bidirectionally is called out explicitly (not
new behavior, but a promoted guarantee).** `rclone bisync` has mirrored
deletes since C2 — this was never new — but it was only implicit in "bisync
gives us correct bidirectional semantics." Given how consequential a
surprise delete would be for a personal knowledge base, C3 makes it an
explicit, tested guarantee (`create_and_delete_propagate_bidirectionally`)
and documents it as user-facing behavior rather than an incidental property
of the transport, guarded the same way as any other change: `--max-delete`.

**Why selective sync (`exclude`/`max_size`) defaults to off.** The user who
prompted this work had opted, deliberately, to sync everything in `~/brain`
including large media; the fix for *that* case was excluding those specific
paths on that one machine, not changing brain's default behavior for
everyone. `SyncConfig::exclude`/`max_size` default to empty, so an
unconfigured brain keeps syncing everything exactly as C2 shipped it — this
is an available knob, not a behavior change.

## C3 — id-keyed CSV semantic merge for tasks/habits (over keep-both)

**Why id-keyed 3-way merge instead of keep-both, for these two files only.**
Keep-both (the C2 default for every other file) is the right call for prose:
losing an edit on a personal note is worse than a little clutter. But
`tasks.csv`/`habits.csv` are edited constantly and are *structured, row-id-
keyed* data, not prose — the worst fit for whole-file keep-both, which would
turn "I completed a task on my phone while you added one on your laptop" into
a `(conflict …)` copy the user has to manually reconcile by hand, every
time. An id-keyed 3-way merge lets that case (and delete-vs-edit, and
different-field edits) converge automatically instead, so the two CSVs never
produce a conflict copy at all — see the design spec
(`docs/superpowers/specs/2026-07-25-brain-sync-c3-csv-merge.md`) for the full
write-up.

**Why the merge uses each row's `last_touched` column.** Same-field
last-writer-wins needs a per-row modified timestamp, and `tasks.csv` already
had `last_touched` for chronic-ignore detection. C3.3 extended that same
column to `habits.csv` and audited the bundled writers so every task/habit
row mutation stamps the changed row before writing. That gives both CSVs the
same recency semantics without adding a sync-only metadata file.

**Why the lexicographic tiebreak still exists.** A first sync may encounter a
legacy or hand-authored CSV without a usable `last_touched` value. Rather
than abort or let side ordering decide the winner, the merge picks the
lexicographically-greater cell value deterministically and journals it as a
soft conflict. That fallback is for damaged or pre-C3.3 rows; normal task and
habit rows resolve by timestamp.

**Why convergence and idempotency are load-bearing properties, not nice-to-
haves.** Two machines must reach the *same* merged file regardless of which
one is "ours" and which is "theirs" in a given run (convergence) — otherwise
each sync could re-diverge the two sides instead of settling them. And
merging an already-merged table with itself must be a no-op (idempotency) —
otherwise a sync with nothing new to reconcile could still perturb the file,
churning it forever. Both are asserted directly as unit tests in
`csv_merge` (`convergence_swapping_ours_and_theirs_is_byte_identical`,
`idempotency_merging_a_merged_table_with_itself_is_a_no_op`), because the
whole scheme's safety — no silent divergence, no human ever needing to
adjudicate a task-CSV conflict — depends on the merge being a genuine
mathematical convergence, not just "usually agrees."

**Why immutable UUID wins over mutable display ID.** Two machines can allocate
the same `T###` or `H###` independently, so display identity cannot safely be
the permanent row key after schema migration. UUID-distinct rows both survive.
For a contested label, the lexicographically smaller UUID retains it; loser
UUIDs are sorted and assigned above the maximum number visible in base, local,
or remote. This side-independent rule makes mirror-order merges byte-identical.

**Why relationships resolve before display reconciliation.** A remote child's
`blocked_by=T10` means the remote `T10`, not whichever UUID later wins that
label globally. Each side therefore resolves `blocked_by` labels and bounded
task IDs in free-text `see_also` to UUIDs before row merge, then emits final
display IDs afterward. The `see_also` scanner skips `http(s)` URL spans and
preserves every separator and non-reference character, including longer IDs
that only contain the changed label. A missing target emits its original display label, never an
internal UUID marker. Project metadata reverse
links are regenerated from the authoritative CSV `project` column for the same
reason. All metadata rewrites are parsed and staged before local publication,
so one malformed project cannot partially rewrite unrelated projects.

**Why schema-v2 validation is one whole-operation preflight.** Validating or
publishing one CSV at a time could update tasks before discovering that habits
is incompatible. Brain validates the manifest and all six base/local/remote
tables first, then stages all project metadata. Any failure therefore leaves
both CSVs, both baselines, metadata, remotes, and counters unchanged. Current
identity requires `task_uuid`, `task_id`, `assigned_to`, and `system_key`;
`last_touched` improves conflict resolution but is not identity.

**Why schema-v2 unknown columns are opt-in.** Silently accepting a column Brain
does not understand can preserve bytes while breaking relationship or identity
semantics. Current tables require the known identity columns. A manifest may
explicitly set `forward_compatible_columns: true` when byte preservation is
safe; otherwise unknown columns refuse before any CSV or baseline write.

**Why `brain check` reports CSV row deltas instead of simulating the full
merge.** `check` is a read-only "what would move?" report, and its value is
fast, low-risk visibility before running `brain sync`. For the task CSV lane,
that means comparing each side to the cached baseline and reporting added,
changed, and deleted rows. Running the full merge in check output would expose
merge adjudication details without committing the merged state anywhere, which
could confuse the contract: `brain sync` is still the only command that applies
last-writer-wins, writes both sides, and refreshes the baseline. Row deltas are
enough to prevent the old blind spot where task/habit edits were invisible to
`brain check`.

**Why `brain check` treats CSV pull rows as baseline diffs, not provenance.**
The CSV lane has no per-machine author field; its source of truth for "what
changed" is this machine's cached last-synced baseline. That means a pull row
in `brain check` literally means "the remote CSV differs from this machine's
baseline", not "another machine definitely made this change." To keep first-run
and repaired-baseline cases from looking absurd, `check` has a small read-only
heuristic when the baseline is missing: if local and remote CSVs are identical,
it reports no CSV movement; if both are non-empty and differ, it treats the
remote CSV as a provisional snapshot for local deltas. The real sync still does
the full id-keyed 3-way merge and refreshes the baseline; this heuristic only
makes the preview less misleading.

## C4 — event-driven auto-sync (`notify`, freshness gates, and the sync lock)

C4 makes sync automatic. The durable choices below are the ones a later phase
(such as a standalone daemon) should keep rather than "simplify."

**Why the watcher is an in-process shell thread, not a daemon.** The parent sync
spec frames C4 as "TUI lifecycle hooks + debounce": the watcher's lifetime is the
shell's lifetime. A standalone always-on daemon would mean a whole second
lifecycle to build and own (spawn, a PID/port record, `status`/`kill`,
stale-record reaping), i.e. the `src/server/` machinery duplicated for sync,
which isn't worth it when the persistent shell is the default `brain` invocation
and is almost always open. Folding the watcher into the brain HTTP server instead was
also rejected: it would couple sync to the `/habits` server being up (it can be
killed independently) and mix two unrelated concerns in one daemon. The accepted
tradeoff is no live sync while *no* shell is open. The next startup pull and
manual `brain sync` cover that gap.

**Why there is no idle or exit sync.** A periodic pull performs network and
filesystem work without a user event and made a long-running receiver machine
harder to reason about. Exit sync is redundant once local writes are watched,
and it complicates shutdown. Downstream work now has two deliberate triggers:
startup, and the moment an inbound receiver message is about to run when the
last successful downstream sync is over two hours old. The two-hour value is a
freshness threshold, not a timer.

**Why `notify`, with a macOS polling fallback.** `notify` provides one watcher
boundary across platforms. Linux uses its recommended native backend. In the
real-filesystem integration test, macOS FSEvents silently delivered no event
for valid changes, so macOS uses notify's one-second `PollWatcher` instead of
pretending an unreliable native watcher is active. We still keep the
three-second burst debounce in our own tested pure `Debouncer`, and depend on
neither `notify-debouncer-full` nor `notify-debouncer-mini`. A standalone
Watchman service would add installation and lifecycle management without
improving the behavior Brain needs while its persistent shell is open.

**Why watcher pushes are one-way and non-deleting.** A watcher-triggered
`bisync` can download unrelated remote changes and write merged CSVs locally,
which violates the downstream trigger policy and re-arms the watcher. Automatic
push therefore uses `rclone copy --update`: it uploads additions and edits,
never downloads, and never deletes remote-only paths. Task CSV and counter
passes may read remote state to preserve remote rows/maximum counters in the
upload, but do not write it locally or advance the downstream baseline.
Deletions reconcile at the next explicit bidirectional or pull-biased sync.

**Why triggers skip rather than queue (coalescing).** Triggers are frequent and
idempotent, so when a sync is already running the in-flight run (or the next
debounce fire) will pick up whatever changed; queuing would just stack redundant
rclone runs. So `try_acquire` is non-blocking and a caller that can't take the
lock simply skips. The watcher's `Debouncer` re-arms after a skip, so pending
changes aren't stranded. One-way watcher pushes write no local files, so they
cannot create a feedback loop.

**Why the advisory sync lock is keyed by workspace UUID.** Concurrent triggers
are the norm: shell start + the watcher + a second shell + a manual sync can all
target the same workspace, and two `rclone bisync` runs for that workspace must
not overlap. A generation-tagged owner file at
`<workspace-cache>/sync/sync.lock` is prepared and synced before an atomic
same-directory hard-link publishes it, then reaped when stale (owner PID no longer alive
via `kill -0` or heartbeat mtime older than the stale cap), gives "one sync at a
time per workspace" cheaply, while allowing two different workspace UUIDs to
sync concurrently. The heartbeat is the minimal
extra mechanism needed to avoid the SIGKILL + PID-recycle wedge: a real long
sync keeps refreshing the lockfile mtime, but a stale lock left behind by a dead
process stops refreshing and becomes reapable even if the old PID number later
belongs to an unrelated live process. Stale takeover first advisory-locks the
observed inode and rechecks its generation, so competing reapers cannot unlink
a successor. Crucially the lock wraps **all** sync entry
points, including the manual command path in `src/command/sync.rs`, setup's
identity/credential/baseline stages, and the migration schema transition. This closes a latent
C2/C3 race that existed before C4: two concurrent manual `brain sync`
invocations could previously collide, and now the second cleanly skips. Manual
sync deliberately skips-with-a-message rather than blocking; a short
blocking-wait for the human path is a noted possible refinement, not shipped.

The UUID boundary applies to the complete sync runtime, not only the advisory
lock. Journal and current-state reads, the follower log, rclone's workdir, and
semantic CSV baselines all derive from the same selected `WorkspacePaths`.
Keeping one path authority prevents a default-workspace change or convenience
HOME lookup from redirecting observation or transport state across workspaces.

## C5 — structured conflict list + brain-side deleter for agent-driven resolution

C4 made syncing itself automatic; C5 closes the remaining manual step —
resolving a keep-both conflict — for an agent rather than only a human at the
terminal.

**Why a distinct `/second-brain cloud-sync`, not overloading `/second-brain
sync`.** *(Superseded by the "sync" ↔ "reindex" rename below — kept for the
historical reasoning.)* At the time, `/second-brain sync` already meant
something: rebuild the derived lookup CSVs
(`projects-lookup.csv`/`zotero-lookup.csv`) from `.METADATA.json`. Routing
"sync my brain across machines" through that same name would silently
repurpose an existing, muscle-memory trigger phrase and make either request
ambiguous. A new, more specific name (`/second-brain cloud-sync`) kept both
intact and let the skill ask a clarifying question on genuinely ambiguous
phrasing instead of guessing which one the user meant.

**Why a structured list (`conflicts --json`) plus a brain-side deleter
(`resolve`), not pure prose.** An agent resolving conflicts needs to know,
unambiguously, which files are copies of which original, and needs a safe way
to remove them once merged — parsing the themed human list or hand-rolling
`rm` on the wrong file would risk deleting a canonical file or leaving a
half-merged group. `brain sync conflicts --json` gives an agent a stable,
tested schema (`ConflictGroup`/`ParsedCopy`, rendered by the pure
`conflicts_json`); `brain sync resolve <original>` gives it a single
brain-owned operation that only ever deletes files it can prove are conflict
copies of that original (via `copies_for_original`), never the original
itself.

**Why `resolve` refuses when the canonical is missing.** A mistyped or
already-deleted original with copies still on disk is exactly the case where
blindly deleting "the copies" is most dangerous — if the canonical is gone,
one of those copies may be the only surviving version. `resolve_decision`
special-cases this as `CanonicalMissing` and refuses outright (in preference
over silently treating it as `NoCopies`), forcing a merge into the canonical
first.

**Why `resolve` is a pure local delete with no sync of its own.** Folding a
push into `resolve` would mean every one of N originals in a batch triggers
its own `rclone bisync`, and would make the command's success depend on
network/lock state that has nothing to do with "did the delete happen."
Keeping `resolve` fs-only (no rclone, no journal entry) makes it fast,
deterministic, and easy to test hermetically; the `/second-brain
resolve-conflicts` skill runs exactly one ordinary `brain sync` after
resolving every group, so the deletions still propagate.

**Why C5 only resolves prose keep-both copies, not CSV soft-conflicts.** The
two shapes aren't the same problem: `tasks.csv`/`habits.csv` already merge
automatically (C3's id-keyed 3-way merge), and a leftover disagreement there
is a same-field tiebreak, not a file an agent could "merge and delete" —
it's already resolved, just noted. Scoping `conflicts --json` and `resolve`
to the friendly `(conflict …)` file copies keeps both surfaces simple; a CSV
soft-conflict stays visible only in the sync journal's `csv:` note (see C3
above), which is the right audience for it.

## Why `Ctrl-N` sends `/new` instead of being forwarded to the agent

Starting a fresh conversation is a frequent gesture, and typing `/new` by hand
each time is friction. `Ctrl-N` is intercepted before the brain-panel key
forwarding (like `Alt+U`/`Alt+D`) and calls the selected controller's
`start_new_session`, so it works
from either panel without first focusing the brain panel. We only intercept it
**while the panel is open** — there's nothing to send to otherwise — which
conveniently leaves `Ctrl-N`'s search meaning (move-down) intact when the panel
is closed and search is full-width. A brand-new `--session-id` isn't used
because `/new` is what makes Claude rotate its own id, which the authorized
SessionStart lineage then records as the session to resume next time (the same
path `/new`-typed-by-hand already takes). The frontend adapter owns the complete
new-session input sequence, so App contains no agent-kind key switch.

Injected work for an already active turn still needs a small timing gap between
text and the final queue action: frontends can coalesce a byte burst ending in
a submit key into one paste. `AgentController::queue_after_active_turn` types
the text and owns a two-event-loop-tick pending semantic action. Claude's
adapter translates that action to `Enter`; Codex translates it to `Tab`.
Shutdown cancels pending controller input, so a closed panel cannot receive a
late submit.

## Why personalization is just another brain config (in the brain root)

Personalization (name, role, who you work for, tag styles, namespaces) is
content *about you* that should be identical on every machine. It is stored beside
`config.json` in the brain config dir (`<brain-root>/.config/personalization.json`,
`settings::config_dir()`) — just another brain config, riding along when the
brain dir syncs. See the config-location decision above for why everything brain
persists lives inside the brain root (and the `root`-pointer exception).

## Why tag styles (and identity) are personalization, not hardcoded

The public repo must carry no personal taxonomy. The task renderer's tag →
emoji+label map used to be a hardcoded `match` full of one user's tags
(`ceo`, `aa`, `mit`, …). Now the binary ships only a tiny universal default set
(`mit`, `personal`, `work`) with a raw-name fallback, and every other tag is a
user override in `tag_styles`. Same for identity: a skill's generic *rule*
("act as a personal assistant") stays in the skill; the personalized *who* it
serves ("a CEO at Avandar") is personalization the skill looks up. This is the
hybrid model — identity is a runtime lookup (`brain personalize show`), so it
changes instantly without a rebuild, while structural per-user variation is left
to the skill render pipeline.

## Why mutations call `resync_skills()` (historical seam, now active)

Any `config set` / `personalize set` / onboarding change should keep the
installed skills consistent with the user's values, so every mutation path
calls one `skills::resync_skills()` hook. That hook began as the rollout seam;
it now runs the deterministic render/install pipeline. It remains best effort:
a `config set` succeeds even if rendering fails, while the unchanged version
stamp lets a later invocation retry.

## Why the renderer resolves tags via a process cache, not threaded state

`type_label` is on a hot render path with two call sites whose callers fan out
widely. Rather than thread `&TagStyles` through every render signature, the
user's styles load once into a process cell (`personalization::runtime`) at
startup. The *decision* logic (`TagStyles::label`) stays a pure, unit-tested
function; the cell is only the data supply. Until it is initialized (i.e. in
unit tests) it falls back to the generic defaults, so tests never see the dev
machine's personalization and stay hermetic. The running TUI reads styles at
startup; changing personalization takes effect on the next launch.

## Why bundled skills are embedded in the binary (`include_dir`)

brain is meant to be cloned and used by anyone, and its skills must work in *any*
agent session, not just inside the repo. Embedding the `skills/` dir into the
binary with `include_dir` makes it self-contained: `brain skills sync` writes the
skills out wherever they're needed, so a user who `cargo install`s brain (or
moves the binary) still gets them. `include_str!` can't carry a skill's multiple
files (SKILL.md + scripts), which is why the one dependency is justified.

## Why the skill install is a two-hop link (registry → built)

`brain skills sync` writes a built copy, links `~/.agents/skills/<name>` at it,
then links each frontend's skills dir at that registry entry. This mirrors the
fan-out shape a dotfiles manager already uses for its own skills, so brain-owned
skills sit in the same shared registry every frontend reads — and so brain and a
dotfiles manager can coexist on one machine once the dotfiles manager stops
pruning brain-owned entries (the B4 bridge). The link *targets* are a pure
function (`layout::link_ops`), unit-tested; the FS shell (`install`) stays thin.

## Why skill auto-sync had a rollout gate (historical; default now on)

During sub-projects B1 through B3, `resync_skills()` was gated off because the
live registry still had another owner and the render/install pipeline was not
ready. The B4 cutover completed that ownership transition and activated the
same seam.

`skills_auto_sync` now defaults to `true`: config/personalize mutations and the
first ready-workspace invocation after a version change render the live
registry. Setting it `false` leaves only explicit `brain skills sync`.

## Why a version-stamped auto-resync (a brain update must ship skill changes)

A bundled-skill change is worthless until it is *rendered* into the registry the
LLM reads. Before this, the only triggers were an explicit `brain skills sync`
or an incidental config/personalize mutation, so after a plain `git pull` +
rebuild the installed flattened skills silently lagged the binary: the code half
of a change went live, the skill half did not (this is exactly what stranded the
daily-triage completion-signal fix on an old render). We close the gap by making
a **version change** a render trigger, entirely in code (no LLM): `bootstrap`
stamps `env!("CARGO_PKG_VERSION")` per workspace (`state` DB
`meta('skills_synced_version')`) and, on the first ready-workspace invocation
after the stamp differs, runs the same pipeline once and re-stamps
(`needs_resync` is the pure decision). Key choices:

- **On any workspace-opening command, not just the TUI or an explicit sync.**
  The trigger the user wants is "a new brain version ran," which is every real
  command; `--help`/`--version` (no workspace), the internal hook/server, and
  registry-only maintenance structurally have no ready workspace, so they are
  the natural exclusions — no special-casing needed.
- **Per-workspace stamp in the state DB.** The flattened render depends on the
  *selected* workspace's extensions/plugins, and the state DB is already
  UUID-scoped, so the stamp belongs there (a generic `meta` key, no migration).
- **Reuse `skills_auto_sync` as the opt-out.** Both auto-render triggers are
  "auto-sync skills"; one knob keeps the mental model simple. `false` ⇒ manage
  the registry only via explicit `brain skills sync`.
- **Extension-agnostic, per [AGENTS.md](../AGENTS.md).** The mechanism knows
  nothing about what any extension renders; an empty extension set renders
  identically, so the bundled core and any fork behave the same. Every
  authoritative render path (version-resync, mutation `resync_skills`, real
  `brain skills sync`) writes the stamp so none re-fires; a `--root` sandbox
  sync writes none. A failed resync leaves the stamp untouched (next invocation
  retries) and never fails the command that triggered it.

## Why extensions inject at named hooks (not append-only, not runtime lookup)

A user must be able to personalize a bundled skill without forking it — and
sometimes that means running something *at the start* of the skill (Pablo's
`triage` calls email-triage first), which a trailing "append" can't express and a
pure runtime lookup can't order reliably. So base skills declare named markers
(`<!-- brain:ext hook -->`) and the extension file supplies content per hook,
substituted in place. Content with no matching marker still lands in a trailing
"Personal extensions" section, so nothing the user wrote is silently dropped. A
skill with no extension renders unchanged (markers are stripped).

## Why optional agenda content is caller-supplied at a generic hook

The bundled todo workflow must remain useful with no personal extension and
must not encode a particular inbox, service, or staging layout. It therefore
declares `todo:agenda-after-build` as a no-op-by-default hook. An installed
extension may invoke the generic optional-content helper, but the caller must
provide the agenda and content paths explicitly. Core performs no source
discovery and attaches no provider-specific meaning to that content.

## Why extensions render a new built copy, never the repo/plugin source

The repo must stay 100% generic and a plugin is the user's own artifact; neither
should be mutated by personalization. So injection happens only when writing the
*built* copy that Claude/Codex read (`render` → `install`), leaving the source
pristine. This is the structural half of the A hybrid model (identity stays a
runtime lookup; behavior changes are rendered), and it keeps `git status` on the
repo clean no matter how heavily a user personalizes.

## Why the bundled `triage` skill drops email/Linear/Notion into extension hooks

Migrating `triage` (B3) forced a line between generic triage logic and one
person's tool stack. The rule we settled on: a *generic workflow* stays in the
core skill (past-due grouping, at-risk scan, chronic-ignore sweep, the
scratch-inbox sweep, the monthly backlog review, the AskUserQuestion protocol);
an *identity fact* becomes a runtime `brain personalize show` lookup ("busy CEO"
→ role/works_for); and a *specific external tool or private URL* moves to the
user's extension. So the whole email-triage-first pass, the Superhuman
reply-reconcile, the Linear reconcile/mirror-in/grooming pass (it hardcodes an
owner email — which personalization has no field for — plus an `AVA-###` prefix
and a `/linear-pm` dependency), and the private Notion In-Basket URL all live in
Pablo's `triage` extension, not the repo. The core declares seven hooks
(`triage:daily-open`, `triage:daily-subagents`, `triage:daily-linear`,
`triage:daily-merge`, `triage:daily-required-outputs`, `triage:weekly-inboxes`,
`triage:weekly-linear`) at the exact points those passes ran, so the rendered
copy reproduces the original flow byte-for-behavior while the repo stays generic.
The `daily-subagents` / `daily-merge` pair is the generic seam for running an
extension's work (e.g. the email pass) **in parallel** with daily triage instead
of serially before it: the core launches registered sub-agents at the start, and
requires all of them to finish and merge their output into the run's output
before Step 9.

**Gating the tab-close on the run's declared outputs (extension-agnostic).**
The daily-triage tab used to close the instant the `/triage` skill POSTed its
one-time token, and in practice the model fired that POST as soon as the *task*
passes finished — before an extension's printable/PDF was baked — so the tab
died mid-bake and the output never landed. The tempting fix (have the code wait
for the PDF) is exactly wrong here: the agenda, the markdown, and `~/Downloads`
are all a *user extension's* concern (`triage:daily-merge`), and the core skill +
`triage_signal.rs` must assume **nothing** about whether any such extension
exists or what files it writes. So the fix is a generic contract: the completion
POST carries a `require` list of output paths *the run itself declared* (an
extension supplies them at `triage:daily-required-outputs`; core supplies none),
and `App::tick_triage_done` holds the signal and refuses to close until every
listed path exists (`triage_signal::ready_to_close`, pure). An empty list —
the no-extension / fork case — closes immediately, identical to the old
behavior. This is the reference case for the extension-agnostic rule now written
into [AGENTS.md](../AGENTS.md): skill-related code and core skill text may assume
a hook *might* carry extension content, never what it contains, and every
generic mechanism must no-op when no extension contributes.
Keeping personal tokens out of a bundled skill is a **review step, not an
automated test** — see "Why there is no automated personal-data guard test"
below.

Cross-skill script calls (todo's `find_chronic_ignored.py`, …)
standardized on the install-registry path `~/.agents/skills/todo/scripts/<name>.py`
rather than the old `~/global-skills/...` (dotfiles-manager-owned) or
`~/.claude/skills/...` (one-frontend) forms: that path is frontend-agnostic and is exactly where
`brain skills sync` installs the `todo` skill, so it resolves for any cloner.

## Why second-brain split into a lean core + a `/contacts` skill + a `zotero-sync` plugin

The original `second-brain` skill was 1200+ lines that fused three concerns:
generic PARA knowledge management, a full Zotero reference-manager integration,
and a local contacts book. Migrating it (B3) separated them along ownership
lines:

- **Core `second-brain`** keeps only generic PARA (buckets, decision flow,
  naming, IPs, project lifecycle, add-resource, extract-IP, sync, CSV tooling).
  The "how to summarize" step delegates to `/article-summarizer`; Zotero-specific
  commands are gone.
- **`/contacts`** is now its **own bundled skill** (generic CSV book +
  `contacts.py`), so it loads only when the user actually asks about a person —
  second-brain just points at it. Its Notion "Our People" fallback is a personal
  extension (`contacts:fallback`).
- **`zotero-sync`** is a personal *plugin* (not bundled): every Zotero command,
  the richer additions table, reading-state retrieval, and the `zotero-*.md`
  references. It builds on core second-brain + `/article-summarizer`.

**Namespaces became runtime config, not an extension hook.** An earlier plan put
the user's project namespaces (`work`/`personal`/…) behind a `second-brain`
extension hook, but namespaces (and the task-tag set) are better modeled as
first-class personalization surfaced in onboarding and editable via
`brain config set namespaces|tags`. So core second-brain reads the configured set
at runtime (`brain personalize show`) with generic example namespaces in the
prose, and declares no namespaces hook — only `company-context` (load a company
profile note) and `reference-manager` (hand off to a reference-manager plugin).

## Why the `todo` migration split off Linear + a whole personal scheduling layer

`todo` was the largest, most-entangled skill (1880 lines) and carried *two*
deeply-woven personal subsystems on top of the generic task core. The split:

- **Generic core** keeps the task system (schema, commands, agenda ordering,
  habits, chunked tasks, backlog, sync), including the `linear_issue` column as
  **inert external-issue-link plumbing** (kept named `linear_issue` so the live
  tasks.csv + `set_linear_issue.py`/`list_linked_tasks.py` scripts don't churn)
  — but nothing in core contacts an external service.
- **Linear → a `linear-sync` plugin** (not a single-file extension, because its
  `linear-link.md` playbook is a reference file — same reasoning as
  `zotero-sync`). Core exposes a `todo:linear` hook + a `todo:linear-backlog`
  hook; the personal `todo` extension fills them by pointing at `/linear-sync`.
  The `triage` extension's `linear-link.md` refs were repointed to the plugin.
- **The agenda's personal scheduling layer** — Google-Calendar busy-block pull,
  the work-hours cutoff + late-work streak, and Pablo's specific daily anchors
  (plus his work-vs-personal `task_type` taxonomy) — moved to `todo:calendar`,
  `todo:cutoff`, and `todo:anchors` hooks. Core agenda is calendar-optional and
  anchor-agnostic: it orders tasks/habits and writes the PDF, pulling busy
  blocks only if `calendar_id` is set and applying a partition/cutoff only if an
  extension defines one.
- Two config vars back this: `agenda_dir` (default `~/Downloads`, so the PDF
  destination isn't hardcoded) and `calendar_id` (empty = no calendar
  integration). Scripts read `BRAIN_AGENDA_DIR`/`MARKDOWN_TO_PDF` from the env
  with sane fallbacks instead of a hardcoded path into a personal tool install.

## Why there is no automated personal-data guard test

An earlier `bundled_skills_carry_no_personal_data` unit test asserted that no
bundled skill contained any of a hardcoded list of personal tokens — the
maintainer's email addresses, employer name, a private Notion block id, personal
handles, home-dir paths. It was **deleted**, because the test defeated its own
purpose: to check that personal data never lands in this public repo, it
committed that exact personal data into this public repo, in a file every
cloner reads. A grep for the very strings it was protecting would have found
them in the guard itself.

It was also structurally weak. A fixed substring list only ever catches the
tokens someone already thought to add, and it can't catch personal-but-not-
identifying content at all (it would never have flagged "Walk Luna"), so it
bought a false sense of coverage on top of the leak.

**The rule stands; only the enforcement moved.** Keeping personal identity,
private paths, and private URLs out of `skills/` is a review obligation on
whoever (human or agent) touches a bundled skill — read the diff and check it,
the way you check anything else that can't be mechanically verified. If we ever
want automation here, it must live outside the repo (a local pre-commit hook or
a private CI secret list), never as committed test data.

## Why no comments-by-default and no decision log in code

Per the user's house style, new code gets a comment only when the *why* is
non-obvious; the function name + these docs carry the *what*. This repo is
not under git, so there's no PR review, no `.difit/` log, and no changelog
file — `docs/` is the durable record.

## B4 — the dotfiles-manager bridge + live cutover (ownership boundary, prune-safety, rollback)

The B1–B3 pipeline was proven only in a sandbox; B4 is the one phase allowed to
touch the live agent registry. The cutover flips the six migrated skills
(`article-summarizer`, `brain-knowledge-capture`, `contacts`, `second-brain`,
`todo`, `triage`) plus two plugins (`zotero-sync`, `linear-sync`) from
dotfiles-manager-owned to brain-owned, and makes the dotfiles manager delegate to
`brain skills sync` without ever pruning what brain owns.

This section describes the general shape of the cutover for anyone whose skills
are currently owned by a symlink-based dotfiles manager. Brain itself knows
nothing about any such tool; all the coordination is on the dotfiles-manager
side.

**The ownership boundary is the link target, not a manifest file.** A registry
or frontend skill link is *brain-owned* iff it (transitively) resolves under
brain's built dir (`$XDG_DATA_HOME/brain/skills` or `~/.local/share/brain/skills`).
Dotfiles-manager-owned links resolve into its own sources (typically a
`~/global-skills`-style dir or `~/.agents/skills`). This falls out of the two
systems' existing designs and needs no new file:

- A well-behaved dotfiles-manager fan-out only ever creates/repoints/prunes links
  whose target points into its *own* sources. A brain-owned registry entry
  (`~/.agents/skills/todo → ~/.local/share/brain/…`) points into neither, so a
  prune-into-own-sources step leaves it alone and aggregation records it as a
  harmless "conflict" (foreign symlink) rather than clobbering it. A prune/sweep
  of dangling links into the manager's regenerated *mirror* never matches a brain
  link either. So such a prune path already spares brain-owned links *by
  construction*.
- brain's `install::sync` `remove_existing` cleanly replaces the old
  dotfiles-manager-owned symlink at each name, so the cutover is a plain re-link,
  not a conflict.

B4 makes this boundary **explicit and defended** rather than merely emergent: the
dotfiles manager gains a `brain` step that (1) invokes `brain skills sync` before
its fan-out so the registry is brain-populated first, and (2) teaches the fan-out
to recognize brain's built dir as a protected foreign source (a brain-owned name
is never aggregated-over or pruned), backed by a regression test that runs a full
dotfiles sync and asserts brain-owned links survive.

**Cutover steps:**
1. Flip `skills_auto_sync`'s default to `true` (rollout is over; mutations should
   re-render live, per program invariant #5).
2. Snapshot the live `~/.agents/skills` + every frontend skills dir to the
   scratchpad; write a deterministic rollback script *before* mutating anything.
3. Run `brain skills sync` for real: installs the 6 skills + 2 plugins into the
   registry (replacing the dotfiles-manager-owned links) and fans them out to the
   frontends.
4. In the dotfiles manager: add the `brain skills sync` delegation +
   brain-ownership prune guard; remove the migrated skills (`todo`,
   `second-brain`, `triage`, `brain-knowledge-capture`) from its own skills source
   so it stops owning them. Skills that were never migrated stay
   dotfiles-manager-owned; leave any skill a brain plugin is meant to supersede in
   place until the plugin is confirmed to fully replace it.
5. Persist brain's config across machines. **Resolved not by the dotfiles manager
   but by moving the config into the brain root** (`<brain-root>/.config/`), so it
   syncs with the brain and the dotfiles manager never touches it — see "Why
   config lives inside the brain root" above. This sidesteps the mirror-clobber
   footgun entirely (the manager would otherwise have symlinked `~/.config/brain`
   at its regenerated mirror and lost brain's runtime writes on the next sync).
   The one machine-local remnant, the `root` pointer (`~/.config/brain-root`), is
   safe to track in a dotfiles repo, because brain only reads it.

**Rollback:** `scratchpad/b4-snapshot/ROLLBACK.sh` removes brain's built dir and
brain-owned links, then re-runs the dotfiles manager's sync to restore its own
registry (the migrated skills must still exist in that repo's skills source, or be
restored from git first).

## Why the `/habits` route inlines its assets and reuses native completion

The `/habits` page (`src/server/routes/habits/`) is a straight MVC port of the
old Python `habits/server.py`, with three deliberate choices:

- **Assets are inlined, not served.** The frontend lives at the repo root under
  `web/habits/` (`index.html` shell + `style.css` + `app.js`), but the brain
  server has no static-file route, only ingress-scoped habits page and
  completion routes. So
  `view.rs` embeds all three with `include_str!` and fills `{{CSS}}`/`{{JS}}`/
  `{{BODY}}`/… into the shell at render time, emitting one self-contained
  document (exactly what the Python single-file template produced). Keeping the
  three source files separate keeps the CSS/JS editable and diffable; the JS's
  only functional change from the Python original is replacing
  `fetch('/api/done')` with the rendered
  `/local/<exact-live-lease>/w/<selected-ingress>/habits/done` URL.
- **Mark-done reuses brain's own completion, not a reimplementation.** `done`
  delegates to `crate::tasks::complete`, so the web "done" is the same native
  mutation (status, completed_date, `last_touched`, habit recurrence spawn, and
  chunked-task MIT migration) as `brain tasks complete`. The server calls the
  Rust API directly rather than shelling out, so no helper script or Rust toolchain
  is required at runtime.
- **A dedicated `Habit` struct, not the shared `Task`.** `tasks::task::Task`
  deliberately drops `ideal_time` (it is `#[allow(dead_code)]` in the habit
  loader), but the habits view sorts and groups by time-of-day, so it needs it.
  Rather than widen the heavy shared `Task` for one view, `model.rs` defines a
  small, purpose-built `Habit` deserialized straight from the CSV, and confines
  all filter/sort decisions to the pure `classify` (unit-tested against
  hand-built rows).

## Versioning

`brain` uses the Cargo crate version as the single version source. The CLI
prints that value through `brain --version`, `brain -v`, and `brain version`.
Every committed code change should include the appropriate SemVer bump in
`Cargo.toml` and `Cargo.lock`: before v1, additive user-visible features bump
the minor version, while compatible fixes and internal changes bump the patch
version. The project stays in the `0.y.z` line until the user explicitly says it
is ready for `1.0.0`.

## Why verbose logging is opt-in and stdout is suppressed in the TUI

`brain` keeps default runs quiet because most command output is meant for a
human or an agent to read directly. `--verbose` creates a timestamped log file
under `/tmp/` and mirrors those log lines to stdout for short-lived, non-TUI
commands so a caller can capture one stream. The persistent shell is different:
stdout is reserved away from full-screen terminal drawing, so verbose TUI runs
write only to the log file. The tasks command palette carries the discoverable
escape hatch: the command palette's log actions switch the main panel to a
scrollable diagnostic view
directory and the log file with the system `open`.

## Why sync setup/repair are explicit separate states

`brain sync setup` is the only command that enables cloud sync on a machine: it
collects Backblaze credentials, creates the `RCLONE_TEST` guard marker, and
establishes the first baseline. One UUID sync-lock guard spans remote ownership
election through that baseline. The candidate machine-local `sync` block is
written only after the baseline returns `Clean`; attention, abort, and transport
errors leave credentials unsaved, so a manual sync cannot observe partial setup state.
`brain sync repair` deliberately does less: it repairs/re-establishes an existing setup by
recreating the marker and running a resync. Keeping the commands separate avoids
silently enabling cloud sync from a recovery command. The UX rule is that any
sync command run before setup explains which prerequisite is missing and ends
with the exact next command: `brain sync setup`.

## Why syncs run in a detached process (never on a TUI thread), and how a running sync stays observable

The original auto-sync layer ran the `on_start` and watcher syncs on **threads
inside the TUI process**. Two failures fell out of that, both reported from the
field:

1. **Output bled over the TUI.** A sync writes rclone's progress to its
   process's stderr, which still points at the real terminal while ratatui owns
   the alternate screen on `/dev/tty`. The progress lines drew on top of the
   frame and the rows "went out of sync." It looked broken.
2. **Quitting corrupted the sync.** When the shell exited, the sync thread died
   but its `rclone` child was orphaned and kept running, while the `on_exit`
   trigger simultaneously spawned a *second* detached `rclone`. Two concurrent
   `bisync` runs over the same paths left the working state inconsistent, and
   the next `brain sync` aborted with a dead-end `rclone exited with an error`.
   The machine lock didn't help: it keys staleness on the lock holder's PID
   liveness, and the holder was the now-dead TUI process, so the lock was
   immediately reap-able even though the orphaned `rclone` was still writing.

The fix makes execution independent of the shell: every automatic trigger
(startup, watcher, receiver freshness) spawns a detached
`brain --brain <canonical-name> sync --if-idle` child (`process_group(0)` +
null stdio) with the selected UUID in `BRAIN_WORKSPACE_ID`. The canonical name
keeps the selector stable across alias/default changes, while bootstrap's UUID
comparison fails closed if that name is ever rebound to a different registry
record before the child starts. A separate process can't touch the
TUI, and a child in its own process group outlives the shell and a terminal
close. There is no in-process sync path anymore. While the TUI remains alive,
it keeps each child handle in a waiter thread and calls `wait()` so completed
children are reaped instead of accumulating as `<defunct>` processes.

Detaching hid the progress, so a running sync now records selected-workspace
state under `<workspace-cache>/sync/`: a `Reporter` appends every line to `current.log` (and
echoes to its own stderr, which is the terminal for a foreground run and
`/dev/null` for a detached one) and writes a `current.json` marker while it
runs. `brain sync status` surfaces `syncing now …` from that marker; a user-run
`brain sync` that finds the lock held **attaches and follows** `current.log` to
completion (`follow.rs`) instead of the old "another sync is already running;
try again" error. Background triggers pass `--if-idle` so a redundant one
coalesces (exits silently) rather than following.

The remaining lifecycle decisions stay workspace-local. Each TUI owns an
immutable workspace context and exactly its own watcher handle; dropping that
handle sends an explicit stop and joins only its worker. Receiver freshness
remains at the live TUI's queued-job consumption boundary, where it can delay
only that workspace's message. The shared server therefore never becomes a
sync owner. Its status and retry decisions use an injected runtime in tests,
with production bounds of a 250ms poll, five-second launch grace, and three
launch attempts.

## Why brain owns the rclone bisync workdir, and reaps its lock

An interrupted `bisync` (a quit shell, a powered-off machine) can leave rclone's
workdir with a stale lock file or half-written listings that wedge the next run.
brain now pins that workdir with `--workdir <workspace-cache>/sync/bisync`
instead of leaving it at rclone's HOME-dependent default. Two payoffs:

- **Deterministic, reapable state.** Because brain's own workspace lock
  already serializes all syncs, any `.lck` present in that workdir is
  necessarily from a dead, interrupted run — so brain removes it before each run
  (`run::reap_stale_bisync_locks`), preserving the `.lst` baselines.
- **Self-healing after an interruption.** If the baseline is unusable, rclone's
  "cannot find prior … / must run --resync" family classifies to
  `AbortKind::PriorListingMissing`, and the existing one-time auto-resync in
  `sync_once` rebuilds it — so the user never has to know a sync was interrupted
  or run `brain sync repair` by hand. (Pinning the workdir also means the first
  post-upgrade run on each machine starts from a fresh workdir and auto-resyncs
  once, which is exactly what heals a machine already wedged by the old
  concurrent-run bug.)

## Why the id counters are max-merged and floored by emitted IDs

`tasks/.tasks_next_id` and `tasks/.habits_next_id` hold the next integer id to
hand out for a new task / habit. They used to ride the normal bisync lane, which
resolves a divergence by **newer mtime**. That is wrong for a monotonic counter:
if the machine holding the *lower* value wrote more recently, bisync would pick
the lower value, and that machine would then re-hand-out ids the other machine
had already assigned — colliding in the id-keyed CSV merge (two different rows
sharing one `task_id`). So the counters are now excluded from bisync and
reconciled out-of-band by the maximum local and remote value.

Max is the whole counter rule, deliberately. It is stateless (needs no 3-way baseline
like the CSVs), convergent, idempotent, and monotonic, so it can never regress a
counter and never lets an id be reused. UUID collision reconciliation adds one
necessary floor: the successful CSV operation returns one beyond the maximum
emitted display number from its reconciled task and habit tables. Counter sync
consumes those floors without fetching either remote CSV again. Push-only sync
also writes the floor locally before the next allocation. This
keeps the next ordinary writer from reissuing a label that reconciliation just
created. A missing or garbage counter is treated as absent; absent counters
still derive their first safe value from the CSV floor.

The configured legacy-machine join is a special local-only use of the same
rule. Its generic rclone pass cannot carry counters because they are excluded,
and its ordinary task-state lane is intentionally deferred while the local and
remote schemas differ. The migration bridge therefore computes floors from
the exact joined tables, fetches any usable remote counters, and atomically
writes `max(local, remote, joined_max + 1)` only to the local counter files
before the journaled legacy semantic step can complete. It never pushes a
legacy CSV or counter generation. A crash before both writes leaves the step
unrecorded, and replay converges to the same values.

## Why the daily-triage nudge waits for the startup sync

The triage nudge asks "today's triage isn't done — run it now?" based on whether
today's `Morning Triage` habit is completed in `habits.csv`. But that file is
reconciled by the startup sync, and another machine may already have done or
skipped today's triage. If the modal is shown at open — before the sync lands —
it is based on stale local data: the user sees a triage prompt that the incoming
sync is about to render moot.

We considered showing the modal immediately in a disabled "syncing…" state and
then resolving it, but the simpler and better behavior is to **not show it at
all until we know the truth**. On a sync-configured machine, `run_tui`
defers the check: the shell is fully usable at once (no modal to dismiss), and
`tick_triage_gate` runs the real `check_daily_triage` only after the startup
sync completes successfully (detected by a newer clean downstream sync-journal
row). Before evaluating the modal, Brain reloads portable config, applies the
incoming managed-triage policy under the task-store owner, and reloads both
task tables. The modal then appears
only if triage is genuinely still due; if
another machine handled it, it never appears. The gate keys on the journal's
row id rather than the `current.json` in-flight marker specifically to avoid the
"sync hasn't written its marker yet" start-gap: a new journal row is an
unambiguous successful-downstream signal. Push-only and non-clean rows do not
open the gate. If the sync is offline or fails,
the gate remains closed for that shell rather than evaluating stale local data.

The CLI suppression flag and palette toggle share one live process-scoped
field. The refresh gate deliberately stores no alert-state snapshot. Enabling
the alert from the palette while startup sync is pending defers the check;
after a successful refresh, Brain consults the live field against refreshed
config and task state. Disabling it again before completion therefore remains
suppressed, while enabling it cannot create a modal from stale pre-sync data.
Refresh failures still surface as errors and never evaluate the alert.

## Why all task writers share one workspace-scoped owner

Managed-triage reconciliation changes portable config, task and habit tables,
counters, and derived references as one logical generation. A process-local
mutex would not protect the Rust CLI, TUI, server, sync worker, and bundled
Python scripts from one another. Brain therefore uses a SQLite immediate
transaction at the workspace UUID cache path as a stable interprocess owner.
Rust mutation entry points hold it across read, decision, and publication;
portable config read-modify-write and web habit completion do the same. Python
CSV and project-metadata JSON writers hold the same owner, compare current
bytes with their read snapshot, and publish with a file-synced atomic
same-directory replace followed by a parent-directory sync. A stale writer
fails explicitly instead of silently overwriting reconciliation. The native
sync metadata publisher already runs beneath the sync command's owner, and the
managed-triage metadata purge runs inside its authenticated grouped
transaction, so every production project-metadata writer shares this boundary.
## Why shared-server routing starts with a pure lease table

The shared receiver must make a routing decision before it reads a selected
workspace's root, environment, user record, credentials, prompts, logs, or job
socket. A pure `LeaseTable` is the narrowest boundary that can enforce that
ordering. It records verified, live TUI registrations by typed workspace,
lease, and ingress UUIDs, with only the lease-local job socket and receiver
intent needed by the later forwarding layer.

The table keeps ingress catalog entries after a lease is removed or expires.
That preserves the meaningful distinction between an unknown public route and
a known workspace whose TUI is no longer live, while pruning the actual lease
before any route can receive stale socket or PID data. A receiver-disabled live
lease is a third, separate state. The HTTP layer can map all unavailable states
to the required behavior without confusing them internally.

The table accepts an injected monotonic instant and heartbeat schedule. This
makes a one-second production heartbeat and five-second TTL deterministic in
tests, allows final-lease shutdown to be a pure decision, and avoids timing
sleeps or an always-on availability responder. Process election, socket IO,
and the watchdog remain separate thin layers built on this state machine.
Ordinary mutating and routing transitions prune expired leases opportunistically
before acting, while immutable status projections only filter them from the
reported view. The watchdog owns the periodic guarantee: it expires a crashed
final lease and shuts the process down even when no request arrives.

## Why every HTTP route resolves opaque ingress before workspace state

Names, roots, defaults, and query parameters are mutable selectors and cannot
safely identify a workspace at a machine-wide listener. Provider endpoints use
`/w/<opaque-ingress>/{sms,email}`. Local habits and triage actions use
`/local/<exact-live-lease>/w/<opaque-ingress>/...`, so a whole-port provider
tunnel cannot publish local reads or mutations. The pure router parses these
capabilities before any handler runs.
Global, malformed, missing, extra-component, or unknown routes are rejected;
there is no fallback to the machine default.

The shared route boundary next asks the `LeaseTable` for a live lease and
captures its exact authority in a generation-bound ticket. Only after that
decision does it release the control mutex, reload the lease's canonical
registry record, check the workspace UUID and root, reopen the portable
manifest, and check both workspace and ingress UUIDs. It then reacquires the
mutex and rejects the result unless the same lease authority is still
accepting. The ticket includes a monotonic per-workspace authority revision:
heartbeat renewal preserves it, while registration or an enablement change
advances it. Removal or expiry leaves no accepting authority, and a later
registration advances the remembered revision. Thus an identical
disable/re-enable or unregister/re-register sequence cannot revive authority
captured before revocation. Revision advancement is checked before any lease
field changes, so overflow leaves enablement, expiry, registration state, and
the current revision unchanged. Handlers receive the resulting
`WorkspaceContext` explicitly. They
never reopen a global root or choose a workspace independently. This ordering
prevents slow filesystem IO from blocking heartbeat or shutdown and prevents
one route from selecting another workspace's tasks, triage signal,
credentials, users, prompts, logs, or job socket. Disabled and no-live-TUI
routes return 503 before handler behavior; receiver dispatch never accepts
unavailable work.

Routing precedes body IO as well as workspace-specific state. Local action
bodies are capped at 16 KiB. `tiny_http` 0.12 internally owns an accept thread,
a task pool that may spawn when no task is waiting, and a request queue whose
capacity constructor is only an allocation hint; its public listener API does
not expose limits for those mechanisms. The shared process therefore uses a
small connection-closing `std` HTTP transport with four fixed accept workers,
no application request queue, a 16 KiB request-head limit, and a two-second
absolute parse deadline. Local routes keep that deadline through response
flush. Receiver bodies plus local signature/event parsing stay inside it, then
successful local verification starts one fixed 30-second phase for bounded
provider retrieval, job acknowledgment, and response. Before enqueue Brain
requires five seconds to remain. Successful progress never renews either
phase, so drip progress cannot retain a worker indefinitely. The parser
accepts either one `Content-Length` or exactly one supported `chunked` transfer
coding, rejects ambiguous framing and invalid field names, and bounds and
validates chunks and trailers. Header values trim only HTTP optional whitespace
(`SP` and `HTAB`) and reject forbidden controls or Unicode whitespace before
framing interpretation. Chunk extensions are rejected as a deliberate,
extension-free safe subset. A start gate lets workers accept only after all
spawns succeed, so partial startup rollback cannot consume a request body. A
stalled client can occupy one bounded worker only until the absolute deadline
and apply backpressure, but cannot occupy the lifecycle loop, grow an unbounded
thread set, or make the control socket
unresponsive. Final process exit signals the workers but never waits to join a
worker held by a client, preserving immediate final-TUI shutdown.

Local URL generation also treats the accepted lease as authoritative. The TUI
retains its verified registration ingress and exact lease capability, and
short-lived commands use a generation-bound control lookup by exact workspace
UUID to obtain both values. Reopening only the
portable manifest could otherwise return a changed ingress, including one
belonging to another concurrently live workspace, after registration had
already accepted a different identity.

## Why the shared server is elected and generation-owned

Several workspace TUIs can start concurrently, but one machine must expose only
one loopback server and one control socket. Startup therefore uses an atomic
`election.lock`: a live process and reachable socket are reused, one lock owner
may remove stale infrastructure and spawn, and losing contenders poll for that
winner within a fixed deadline. A loser returns to election when the observed
token disappears, while an elected parent watches for child failure before
publication and releases its exact token before retrying. A process-scoped
advisory lock on the shared
server directory serializes every exact observed-owner compare/remove/transfer,
so a replacement winner cannot be reaped between validation and mutation. The
starter explicitly hands its generation token to the child before releasing
that mutex, but retains an exact-owner cleanup capability until bounded
publication observation finishes. Child adoption changes the owner identity
and makes cleanup a no-op by comparison; child loss before adoption leaves the
parent token unchanged and removable. Explicit cleanup retries transient
advisory-mutex contention at a fixed interval within a bounded two-second
window, then reports acquisition, removal, or timeout failure to its caller.
This prevents a brief contender from stranding the parent token while leaving
an adopted or replacement token untouched. Cleanup uses fallible inspection
before acquisition and during exact conditional removal: only `NotFound` means
no token, while filesystem and JSON errors propagate. The operation borrows
rather than consumes its handoff value, preserving the capability for a repair
and retry after failure. The hidden `server run` command requires the elected
generation token, so it is not a manual availability surface. Public server
commands are read-only `status` and `logs`; short-lived habits and triage paths
attach to an existing process and never elect one.

The process publishes only PID, port, generation UUID, and start time. Its
owner holds the election lock while generation-checked cleanup removes the
record and socket. This prevents an exiting or signalled stale process from
deleting a newer winner's artifacts. An orderly final unregister exits
immediately; the watchdog applies injected-clock lease expiry and exits after
the final crashed lease reaches TTL. Final-expiry shutdown is latched across
failed late control transitions, and a child without its first registration
exits after a two-second bootstrap deadline. Signal flags and the cleanup owner
exist before state publication, closing the startup signal window. The design
deliberately has no durable inbound queue, replay worker, headless agent,
manual restart, or always-on responder.

## Why status probes bypass ordinary bootstrap and logging

The shared lifecycle acceptance gate treats status as observation, not as an
opportunity to make the selected workspace ready. Ordinary ready-workspace
bootstrap may migrate the registry, initialize access config, recover portable
user transactions, refresh installed skills after a version change, and write
the render stamp. The ordinary run logger also creates a private `/tmp` file.
Those are correct for commands that will work with the workspace, but they
make a status probe mutate the thing it is measuring.

`brain server status` and `brain receiver status -b <workspace>` therefore pass
through a pure command classifier before logger initialization. Receiver status
uses a read-only selected-workspace bootstrap that validates existing registry,
manifest, and users bytes through non-recovering readers. It returns the same
four status facts without migration, repair, lock creation, skill rendering,
stamp writes, process election, or live refresh. When a process record names a
live PID, receiver status sends one generation-bound workspace-status request;
that response supplies both process lease count and exact-workspace state.
Transport, protocol, and generation errors remain errors. The server computes
status through immutable lease-table projections, so status cannot reap TTLs,
advance revisions, or latch shutdown. An incomplete workspace must be repaired
explicitly. This keeps process state and persistent receiver intent observable
without hidden state churn.

## Why control registration reopens authoritative workspace identity

The TUI knows its selected workspace, but the machine-wide server must not
use a client-supplied root, endpoint, or enablement value to select state.
Control registration carries the TUI-resolved root only for an ephemeral
normalized comparison, plus stable identity and the claimed UUID-scoped job
endpoint. The server reloads the machine registry by exact canonical name,
checks the workspace UUID and root, reopens that record's manifest, checks
workspace and ingress UUIDs, and takes receiver enablement from the registry.
It derives the endpoint from its own machine home plus the validated UUID,
requires the claim to match, and proves the matching singleton PID and listener
are live within the control request's deadline. The root is never retained and
only the server-derived endpoint enters the lease. This keeps roots and
credentials out of process state while
preventing a local client from redirecting a lease across silos.

Control frames are newline-delimited JSON with a fixed size cap and one absolute
deadline checked before every connect, write, flush, and EOF-terminated read
attempt, including successful progress. Stable `std` cannot initiate a
cancellable nonblocking Unix-domain connect, so the control plane uses the safe
`nix` socket and poll wrappers. A timed-out attempt drops its owned descriptor;
no detached connector thread can accumulate. The server uses the same bounded
connector for its job-listener liveness probe. Registration and enablement
refresh copy immutable capabilities under the state mutex, perform registry,
manifest, singleton, and socket IO outside it, then reacquire only to check the
generation and original absolute deadline before mutation. Independent bounded
control workers keep status, heartbeat, and unregister responsive. Every mutation names
the process generation, so a heartbeat or shutdown from a dead generation
cannot alter a replacement winner. Startup carries one deadline through
connect, election, and registration, retrying missing or stale generations but
returning authoritative registration rejection immediately. A missed heartbeat
uses the same handshake; an injected scheduler and recovery boundary make
concurrent recovery deterministic in tests. The election lock, rather than
timing, chooses the single replacement.

Registration may commit before its response reaches the TUI. Retrying the exact
same generation, lease, workspace identity, PID, and server-derived endpoint is
therefore idempotently accepted and refreshes the deadline. Treating only this
identity-exact replay as success preserves the duplicate-workspace and
duplicate-lease exclusions for real contenders.

## Shared receiver admission with TUI-only execution

The machine-wide shared process owns HTTP admission, but it never provides
offline availability. Receiver ingress must first select an enabled, live TUI
lease. Only then may Brain load that workspace's provider configuration,
authenticate the provider, load its portable users, resolve `ActorContext`, and
forward to the lease's server-derived UUID-local job socket. The shared process
does not start for inbound traffic and contains no durable queue, replay worker,
headless agent, or availability-only responder.

The shared fixed four-worker boundary covers habits, triage, SMS, and email.
Receiver bodies and serialized job frames are limited to 1 MiB, and each TUI's
in-memory handoff queue is bounded at 64 jobs. The socket first holds a decoded
job in connection-local staging and returns `prepared`. Final registry and
exact-revision checks authorize an in-flight admission; only its atomic commit
allows the TUI to append and acknowledge. Disable and unregister cancel pending
or authorized admissions. If commit already won, revocation waits outside the
control-state mutex only until the original request deadline. A timeout rejects
the control request and applies no later lease mutation. Watchdog expiry removes
the exact lease first, preventing new admissions, then cancels every matching
pre-commit admission. Ordinary lease operations filter expiry but never remove
it; shared control and watchdog entry use that single revoke-aware removal.
Final admission performs persisted-intent filesystem IO outside the control
mutex. One combined commit operation then acquires control, samples exact TTL,
revalidates the route and admission identity, and performs the admission CAS
before unlocking. Disabled, missing,
full, and failed endpoints receive one
channel-specific unavailable response and the request is discarded.

The TUI keeps its listener nonblocking so each event-loop poll stays bounded,
but explicitly returns every accepted job stream to blocking mode before
applying fixed read and write timeouts. Some platforms can otherwise surface
`WouldBlock` while a sender is still completing a frame, turning a healthy live
TUI into an intermittent unavailable response. Bounded deadline polling in the
integration suite exercises this boundary without fixed sleeps.

If the TUI appends after `commit` but cannot write its final `accepted`
acknowledgment, it removes that exact staged tail item before releasing its
exclusive queue borrow. The server therefore treats the handoff as failed and
never commits an ID for work the TUI did not acknowledge.

Webhook verification follows provider replay guidance: HMAC comparisons are
constant-time and Resend timestamps have a five-minute tolerance. Provider
delivery IDs use a 1024-entry accepted cache keyed by workspace, channel, and
provider ID. An ID is retained only after a successful enqueue acknowledgment;
failed SMS handoffs release it, and an in-flight duplicate is unavailable
rather than prematurely acknowledged. A known unavailable Resend ingress is
still resolved before credentials; only that routed workspace's signing secret
is then loaded to verify the event. A verified unavailable Resend ID is retained
as a permanent discard, so later TUI availability cannot replay it. This is a
bounded in-memory dedup record, not a queue, replay worker, or headless path.
Persisted disable remains authoritative before live refresh: its failed route
retains exact ingress-to-workspace identity for the same verified discard.

The accepting request captures immutable actor, channel, normalized sender,
response email, and allowed authenticated-thread recipients. The TUI routes
that same context through `AgentController` for Claude and Codex. Configuration
changes during the turn cannot replace the initiating actor or broaden reply
recipients.

The route ticket remains attached to that accepted context. After provider
work and actor/job construction, dispatch reloads the exact canonical registry
record and requires its immutable workspace UUID and persistent receiver intent
to remain valid. It then reacquires the control mutex only to revalidate the
exact generation, authority revision, receiver enablement, and live lease, and
releases the mutex before the UUID-local socket handoff. At staged-socket
commit, persisted intent is reloaded outside the mutex; one combined operation
then locks control, samples exact TTL, revalidates that same authority and the
admission's workspace/lease identity, and performs the admission CAS before
unlock. The attached
authority revision and cancellable admission reject notified,
notification-lost, unregister, and disable-enable ABA revocation without
holding the mutex during provider or socket work.

Persistent receiver intent is the mutation commit point. A generation-bound
live refresh is a convergence notification, not a second transaction: failure
to deliver it is surfaced as a warning while the committed setting remains in
the CLI and palette state. Final admission's authoritative registry reload is
the safety backstop that prevents such a failed notification from accepting
new work.

Resend's two possible Receiving API calls are bounded independently at ten
seconds and 1 MiB inside the 30-second handler total. The parse phase must
still be live before that handler phase can begin. After provider work, Brain
reserves the final five seconds exclusively for the HTTP response and caps the
job handoff at two seconds. One absolute handoff deadline covers the safe
nonblocking connect, full frame write, and acknowledgment read, so successful
progress cannot consume a renewed timeout. One shared compile-time timing
invariant prevents these bounds from drifting apart. The curl reader stops
after one over-limit proof byte and reaps the child before returning a typed
502. Resend receives HTTP success only for verified unavailable, ignored, and
permanent discard outcomes so discarded webhooks cannot be replayed into a
later live TUI; signature failures remain authentication failures, while 500
and 502 remain provider-visible failures. Accepted email jobs
retain stable Resend email and attachment identifiers, and delayed dispatch
refreshes signed download access using freshly loaded workspace credentials.
Processing and final replies preserve accepted subject and message lineage
without widening recipients.

Provider requests still use the system `curl` binary to avoid adding a second
HTTP client stack, but the complete curl configuration is written through the
child's standard input. Secrets, message content, and signed attachment URLs
therefore do not appear in the child process's argument list, and the child
output is captured rather than inherited by the TUI. Outbound replies run on
one bounded background worker. This preserves provider ordering and prevents a
slow Twilio or Resend request from freezing keyboard input or delaying
`Ctrl+Q`.

An open agent PTY is not proof that work is active: brain opens an idle panel
before the startup daily-triage modal. The receiver therefore tracks submitted
turns separately and lets the Stop hook clear that state. Queued receiver work
can replace an idle panel immediately, but never interrupts a submitted local
turn. A receiver launch is committed only after PTY creation succeeds; failure
keeps the message queued and applies a retry backoff.

SMS allowlist comparison uses the provider's exact E.164 sender form. Brain
preserves the leading `+` as string data instead of interpreting it as a JSON
number, recovers the one-number numeric shape written by older releases, and
keeps a yellow TUI status warning visible for malformed configured numbers.
This avoids silently disabling SMS while retaining strict sender matching.

## "sync" means cloud sync; the local lookup rebuild is "reindex"

The C5 decision above kept two verbs alive — `/second-brain sync` (rebuild the
derived lookup CSVs) and `/second-brain cloud-sync` (push/pull files across
machines) — and leaned on a clarifying question when a request was ambiguous.
In practice a plain "do a sync please" hit that clarifier every time, which is
exactly the friction the skill is supposed to remove: to a user, "sync" means
one thing — move my files between machines.

So the bundled `second-brain` skill now reserves **"sync" for cloud sync** and
renames the local lookup/metadata rebuild to **`/second-brain reindex`** (script
`reindex.py`, formerly `sync.py`). The lookup CSVs are derived indexes over the
canonical `.METADATA.json` sources, so "reindex" names the operation exactly and
carries no overlap with the file-moving sense of "sync". A bare "sync" / "do a
sync" now routes straight to `brain sync` with no clarifying question; reindex is
reached only by explicitly naming it ("reindex", "rebuild the lookups", "refresh
the derived CSVs"). The `bundles_the_generic_second_brain_skill` guard test in
`src/skills/embed.rs` pins the new nomenclature (reindex present, `sync.py` and
`/second-brain sync` gone, "do a sync" routes to cloud).

## `brain reindex` is a native Rust command, not a bundled script

The skill had long documented `python3 ~/.agents/skills/second-brain/sync.py`
(then `reindex.py`) for the projects/resources lookup rebuild — but that script
**never existed**. The lookup CSVs were hand-maintained by whatever agent touched
them, which drifted: e.g. `has_other_notes` read `yes` for items whose `notes.md`
said "No standalone user notes attached." Only the task/habit half (the `/todo`
Python scripts) was ever real.

We closed the gap with a native `brain reindex` subcommand (`src/reindex/`)
rather than finally writing the Python script. Rationale: it fits brain's
CLI-for-every-action + Rust-TDD ethos; the pure cores (notes.md scanning,
metadata→row mapping, CSV rendering, selection) are unit-tested and the IO walk
is a thin shell; and it gives themed, narrated output. The `--tasks` half still
shells out to the shared `/todo` rule scripts (the canonical, shared
implementation) rather than re-deriving those rules in Rust.

Field notes that shaped the implementation, all found by running it against the
real brain: resource metadata is **heterogeneous** (`year` may be a JSON number
*or* string; the type is keyed `item_type` *or* `type`; fields may be `null`),
so parsing coerces through `serde_json::Value` instead of a strict struct — a
strict struct silently dropped ~15% of records. `notes.md` placeholders come in
several forms (`*No … attached.*` and `(none)`); the italic wrapper is what
distinguishes a sentinel from a real note that merely starts with "No ".
Directory columns are derived from the filesystem path (authoritative over stale
JSON), rows are sorted deterministically (projects by name, resources by
directory), and output is LF-terminated to match `tasks.csv`/`habits.csv` and the
`csv_merge` writer (the pre-existing lookup files were stray CRLF).

## The daily-triage tab: a dedicated ephemeral slot + a flag-file bridge

Daily triage can be long and interactive, so running it inline in the main brain
session blocked everything else until it finished. We moved it into its own
brain-panel **tab** (`Alt+1` main / `Alt+2` daily triage) so the pass runs as a
background task.

**Why a dedicated `triage_brain` slot, not a general
`Vec<AgentController>` of sessions.** The requirement was explicitly narrow:
users must *not* be able to spawn arbitrary sessions; a second session may
exist *only* for daily triage. A dedicated `Option<AgentController>` plus a
two-variant `BrainTab` models exactly that, keeps receiver/session state
centered on the dedicated `App.brain` controller, and cannot grow into an
unbounded tab manager by accident. Generalizing to N sessions would have been
a much larger refactor for capability we deliberately do not want.

**Why the session is untracked.** The triage tab is ephemeral by construction.
`App::open_triage_tab` builds an `AgentController` from a `LaunchRequest` whose
hook metadata carries only `BRAIN_TRIAGE_DONE_URL` and `BRAIN_TRIAGE_TOKEN`.
The adapter adds the common workspace identity and agent kind, but the request
has no instance ID, state DB, or response ID. The SessionStart hook therefore
never records it, and it is never a resume candidate. If the shell closes
mid-triage the session is lost and the startup nudge simply fires again next
launch, which is the desired behavior.

**Why a completion signal instead of idle-detection, and why via the brain
server.** "The agent went idle" is unreliable because a triage pass asks the
user questions. The `/triage` skill therefore POSTs an explicit completion
signal (with a one-time token) once the pass truly ends. It targets the shared
process already attached to the live TUI; opening a triage tab never elects or
starts a server independently. A localhost `POST
/local/<exact-live-lease>/w/<selected-ingress>/triage/done` carries the exact
live TUI's capability and matches the local habits-completion precedent.
Because the server
is a *separate process* from the TUI, the signal crosses on disk
(`<workspace-cache>/triage-done.json`) and the matching TUI polls it in its
existing per-tick
loop, the same poll-of-disk pattern the triage nudge and receiver responses
already use. The token guard prevents a stale signal from closing a fresh tab.

## Palette commands carry a per-command `is_visible` predicate

Command-palette visibility used to be a single growing `match` in
`PaletteState::scoped` that special-cased each conditional command inline
(`CloseBrain` needs a panel, the receiver rows need a running/stopped server,
the notes/links rows need notes/links). Adding the daily-triage tab-switch rows
would have meant extending that match yet again.

Instead each `PaletteCommand` now carries an `is_visible: fn(&PaletteState) ->
bool` predicate (default `always`). `scoped` applies the *structural* gate
(`command_in_scope`: task-vs-global, the habit filter, the logs-view whitelist,
the task-actions-modal restriction) and then the command's own predicate. The
conditional logic lives next to the command it governs, new conditional commands
are a one-line predicate, and `PaletteState` is the single snapshot of TUI state
the predicates read, seeded at open time from the relevant `App` fields.

**Why the tab-switch commands exist at all.** `Alt+1` / `Alt+2` are the intended
tab switches, but terminal `Alt+digit` handling is unreliable — many terminals
can't distinguish `Alt+1` from a bare `1`, and the encoding varies by terminal
and keyboard layout. In a TUI where a focused brain panel forwards every key to
the child agent, the *reliable* app-level surface is the command palette
(`Ctrl+P` → filter → Enter), so **Show main brain session** / **Show daily
triage session** are the works-anywhere path; the Alt chords remain as a bonus
where the terminal supports them.

## Portable-user removal uses a recovery journal, not absent live files

Removing a portable person can update two assignment CSVs and `users.json`, but
the filesystem has no cross-file rename primitive. Moving live files aside
before replacement left a crash window where ordinary readers found files
missing, and best-effort rollback could silently leave a mixed generation.

The grouped transaction now copies mode-preserving backups, stages and syncs
all replacements, and publishes a strict portable journal before touching live
paths. Each installation is an atomic same-directory rename over an existing
live file; assignment files install first and `users.json` last. An ordinary
error rolls the whole group back and reports rollback failures. If the process
stops after journal publication, the next portable-user load restores the old
generation before parsing it. Journal removal is the durable commit point, so
cleanup after that point cannot change the committed result.

The journal travels inside workspace config because another machine may
encounter the interrupted portable state. Its SQLite serialization lock does
not travel: it is derived from the immutable workspace UUID under the
machine-local runtime cache. This boundary avoids syncing lock state while
still preventing concurrent Brain processes on one machine from publishing the
same grouped mutation.

## A single-user workspace adopts its sole person instead of asking

Readiness once treated an empty machine-local `local_user_id` as a setup gap for
every workspace: interactively it prompted for a user ID, and headlessly it
failed with `brain user local <USER_ID> -b <name>`. That is right when the
choice is real, but it produced a bad experience in the common single-user case.
A workspace that reached the users-present / `local_user_id`-empty state (for
example the first portable person was written but the machine-local link was
not) turned *every* later command into a dead end: running `brain skills sync`
printed `workspace <name> needs setup` and told the user to go run a different
command first. The user had already answered every question brain could
meaningfully ask.

When the manifest is present, exactly one portable person exists, and
`local_user_id` was never set, `readiness_action_with_users` now returns
`ReadinessAction::AdoptLocalUser(id)` in every interaction mode. Bootstrap links
that sole person as the machine-local actor under the registry transaction,
notes it on stderr, and continues the original command. There is nothing to ask
when there is only one possible answer, and a read-shaped command such as
`config list` self-heals the registry the first time it runs. This honors the
house rule that a human should never be sent to `--help` or a follow-up command
to do the obvious thing.

The adoption is deliberately narrow so it never guesses over a real decision. A
*nonblank* but unknown `local_user_id` is left as `InvalidLocalUser` (someone
set it wrong; do not silently overwrite), and two or more people with no local
selection still prompt interactively or fail headlessly with the exact repair
commands. Only the unambiguous case, blank id plus exactly one member, is
auto-resolved.

## Skipping the daily-triage nudge is deterministic, not agent-driven

The daily-triage nudge offers three buttons. **Yes** and the sibling "generate
agenda" flow hand off to the brain panel because they involve judgement (which
past-due tasks to defer/drop, which MITs to pick, back-and-forth with the user).
**Skip** does not: "skip triage today" is pure bookkeeping — mark today's
protected Morning Triage occurrence done and spawn tomorrow's, keyed on the
stable `system_key`. There is nothing to decide.

So Skip no longer injects a natural-language prompt (`SKIP_TRIAGE_PROMPT`, now
deleted) into the main session and rely on the agent to run the `/triage` skill's
skip rule. `App::skip_triage` calls
`tasks::triage_habits::complete_managed_triage(Daily)` in-process and reloads the
tables — no panel, no prompt, no agent, no completion signal (nothing is watching
a tab, because no tab opens). The same entry point is exposed as
`brain habits complete-managed-triage <daily|weekly>`, so the button and the CLI
share one Rust path and an agent can perform the skip non-interactively. Using an
LLM to run a fixed CSV mutation was cost and latency with no upside, and it could
fail or drift; a direct call cannot.

Two invariants keep this honest. It **respects `enable_triage_habits`**: with the
feature off the call is a `Disabled` no-op that reads and writes nothing (so a
fork with the feature disabled behaves identically, and the nudge — which itself
only fires when the feature is on — is still dismissed). And it goes through the
one sanctioned managed-habit mutation path: the ordinary complete/skip CLIs still
refuse managed rows (`protect_system_key`); `complete_managed` is the deliberate,
protection-bypassing exception, mirroring the bundled
`apply_sync_rules.py --complete-managed-triage`.
## Why receiver enablement is persistent intent, not process state

The shared server exists only while at least one workspace TUI lease is live,
so starting or stopping receiver ingress cannot mean starting or stopping a
daemon. Enablement instead belongs to the exact workspace registry record.
CLI start/stop, startup `--with-receiver`, and both command palettes share one
pure transition and one canonical-name plus immutable-UUID transaction. This
keeps CLI and runtime labels in lock-step and prevents a stale selection from
changing a replacement or peer record.

After persistence, a caller may notify an already-running process by generation
and workspace UUID. The process reloads authoritative intent and refreshes only
that live lease; it does not trust a client-supplied boolean. No process or live
lease is a successful no-op because the next registration reads the persisted
value. Route loading also rechecks persistent intent on the exact record before
workspace-sensitive data or the job socket is opened, closing the notification
race. Status deliberately prints intent, TUI liveness, process reachability,
and effective acceptance separately. This preserves TUI-only execution without
inventing a durable queue, replay path, headless agent, manual lifecycle, or
always-on responder.

## Why receiver setup joins machine credentials to portable users by workspace

Provider credentials and public routing origins describe one machine's
connection to one workspace, so setup writes them only into the already
selected schema-v2 machine record. Inbound identity describes a portable
person, so the corresponding phone or email belongs in that workspace's
`users.json`, not in process state or a machine-global allowlist. The setup
planner requires only the address family selected by the channel and carries
an explicit inbound-allowed value for headless parity.

The portable manifest is the sole owner of public ingress identity. Setup reads
its stable UUID to render `/w/<ingress>/<channel>` and never generates or
rewrites it. Only new workspace initialization creates an ingress; attach,
rename, alias, and default changes preserve it. This lets every provider URL
remain stable while mutable machine selectors change.

Guided and headless input converge on one validation boundary before any
write. A public base is an HTTPS origin rather than an arbitrary concatenated
string; selected-channel credentials cannot be blank, and sender phone/email
values are normalized without appearing in an error. Because provider env,
portable users, and frontend hook settings live in separate stores, setup
cannot use one filesystem transaction. It instead snapshots those exact
selected artifacts, holds one persistent workspace-local advisory lock across
snapshot, ordered writes, commit, and bounded rollback, and then releases it.
Acquisition checks its monotonic deadline before every lock attempt and again
before returning ownership. It returns an actionable typed timeout before
snapshot or mutation, including when a free lock is observed at an already
elapsed deadline.
This transaction-wide ownership prevents an identical concurrent after-image
from being mistaken for the failed attempt's bytes. Selected env rollback mutates only the UUID-pinned record, so a
peer workspace update is never replaced by a whole-registry snapshot. Rollback
failures are aggregated rather than hiding the original error.

Only a fully committed setup sends one exact-workspace reload notification.
The notification reuses the generation-bound enablement refresh,
which already reopens authoritative selected state and updates only that live
lease. No setup path elects or restarts the shared process. Since request
handlers load provider and user data only after ingress selection, this reload
mechanism does not introduce cross-workspace cache ownership or weaken the
routing-before-secrets boundary.

Receiver setup places credentials and personal addresses on argv for complete
headless parity, so raw argv is not safe observability data. The logging entry
point centrally redacts those option values and `receiver set` assignments
before both file persistence and verbose mirroring. Env assignments consult the
same `env::is_sensitive` classifier used by config display, including the whole
`agent_capabilities` document and nested MCP credential fields. Names are first
canonicalized exactly as the env command canonicalizes them. Mode-`0600`, exclusive run
log creation is an additional local defense, not a substitute for redaction.

## Why rollout acceptance composes production seams behind fake external edges

A real TUI, PTY, cloud provider, and agent provider would make the rollout
scenario slow, credential-dependent, and timing-sensitive without proving
Brain's own decisions more directly. The Phase 5 acceptance harness therefore
uses real temporary registries, manifests, users, caches, locks, shared-server
control and HTTP routing, task script mutation, CSV reconciliation, triage
configuration, capability planning, and `AgentController`. Only the signed
provider request and agent transport are doubles at boundaries Brain does not
own.

This composition keeps one scenario responsible for the personal-plus-family
lifecycle while focused suites retain exhaustive branch coverage. It also
prevents test convenience from becoming a production acceptance flag or a
second workspace lookup path. Shared-process and watcher transitions wait on
observable conditions with bounded deadlines; they never use fixed sleeps.
The real-rclone complement remains prerequisite-gated and local-only.
