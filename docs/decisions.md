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

## Why the shell owns one overlay enum

Modal state belongs to the persistent shell, not to either main view. Keeping a
separate `Option` for each task modal plus palette and confirmation fields in
`picker::App` made impossible combinations representable and forced input and
drawing to maintain matching precedence chains. The shell now owns one
`Option<Overlay>` whose data-bearing variants cover every existing modal.
Opening, replacing, routing, and closing are explicit transitions, and both the
key router and draw pass exhaustively match the same enum. Task and search
confirmations remain distinct variants because their accepted actions are
different; the single owner does not unify their behavior.

The picker therefore owns search, matching, navigation, and selection only. It
returns the contextual palette or confirmation data that the shell places in
the overlay. A captive modal still swallows the same keys, `Esc` and cancel
still return to the unchanged underlying view, and confirmed actions retain
their existing effects, but there is no hidden second modal waiting behind the
visible one.

## Why shared application actions use a closed enum

The task, log, and brain-search catalogs are contextual views over one
application. Commands such as Message brain, Toggle receiver, Open habits, and
Open agenda therefore use one `GlobalAction` identity and one
`App::execute_global_action` mediator, while `TaskAction` and `SearchAction`
retain task IDs, selected paths, and other feature-only semantics. Each feature
enum wraps `GlobalAction` explicitly, so catalogs stay statically typed without
trait objects or erased callbacks.

Direct key routes obey the same boundary as palette rows. Close brain, Show
tasks, Message brain, and Open agenda all enter
`App::execute_global_action`; the active skill-session form of Close remains a
feature-local tab operation. A structural test inventories these shortcut
routes so adding a direct bypass cannot silently create a second executor.

Both surfaces build the same reusable `PaletteRow<A>` and
`CommandPalette<A>` state. The model centralizes numbering, filtering,
selection, cancellation, and confirmation, while a small controls value keeps
the established navigation and text-input differences between the two
surfaces. That controls value also selects the established filter policy:
case-insensitive word atoms for search, and one case-insensitive contiguous
substring for task palettes. Catalogs still decide row order, dynamic labels,
visibility, task context, and destructive-row placement.

A plugin registry would make a finite in-process command set harder to audit,
weaken exhaustive dispatch, and require erasing feature context. A closed enum
instead makes every application effect visible to the compiler and keeps
cross-feature work at the `App` boundary.

## Why the palette is a modal overlay, not its own screen

The palette is drawn as a modal **overlay inside the persistent shell**
(`menu::draw_modal` over the active view, with the shared `CommandPalette`
driven by the shell's overlay route), rather than a separate full-screen TUI
the way it started.
The reason is `Esc`: a separate screen would have to *exit* on `Esc`,
dropping the user all the way back to the shell and losing the search they
were in. As an overlay, `Esc` just closes the box and the picker is still
right there underneath — the same back-out-of-a-modal behavior the `tasks`
TUI has. This is why `menu/` has no `run()`/event loop of its own; it
exposes a contextual catalog plus `draw_modal`, and the shell owns the loop.
A confirmed row returns a `SearchAction`, which
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

## Why terminal cleanup keeps the established safe order

`TerminalSession` is the sole owner of `/dev/tty`, the ratatui terminal, and
the terminal modes Brain changes. Acquisition records each successful or
possibly partial capability before proceeding, so a later setup failure can
run the same restoration used by orderly shutdown. Explicit restoration is
idempotent, and `Drop` retries remaining best-effort cleanup while logging any
failure without panicking.

The release plan intentionally preserves Brain's existing externally visible
order: pop keyboard enhancement when it was pushed, disable raw mode, disable
mouse capture and leave the alternate screen, then show the cursor. This is a
dependency-safe release of acquired capabilities, not a mechanical reversal
that would reorder established terminal commands during an architecture-only
refactor. If the paired alternate-screen and mouse-capture write fails partway,
both harmless inverse commands are already armed. Tests pin this order through
a headless terminal-operations seam; production still writes the same commands
to `/dev/tty` and adds no capability probe.

Keyboard enhancement remains optional in teardown as well as setup. A failed
pop is logged and stays armed for a later explicit or `Drop` retry, but it is
never returned in place of an event-loop error or a required cleanup failure.
Required cleanup continues through every armed capability, returns its first
error, clears each successful capability, and retries only failures. When both
the event loop and required cleanup fail, the required cleanup error retains
the pre-refactor precedence established by the teardown `?` sequence.

## Why one `TuiRuntime` owns the process lifetime

The persistent shell previously kept its process resources as locals in
`run_tui`, while the event loop named every recurring subsystem. That preserved
behavior but left ownership, recurring order, and teardown order implicit in one
long function. `TuiRuntime` is now the composition root for the already-existing
RAII owners. It owns the App, terminal session, workspace singleton, heartbeat
worker, watcher, periodic puller, receiver endpoint through the App, and shell
instance/session lock. It does not wrap or duplicate their cleanup internals.

Startup uses named stages in the established order. A pure lifecycle model pins
that acquisition sequence and makes orderly shutdown idempotent. Shutdown keeps
the server unregister before agent shutdown, then stops the periodic puller and
watcher, releases the session lock, restores the terminal, and holds the
workspace singleton until the runtime drops. `Drop` invokes the same sequence
as a best-effort fallback, so an early return cannot bypass cleanup. Controller
and session-release failures are logged while later teardown stages continue.

Partial startup needs a narrower ownership rule because registration makes the
shared server advertise the job socket. One production boundary therefore owns
the heartbeat lease before the socket through all later fallible preparation,
including terminal and application setup, the initial panel, and startup
workers. Its ordinary field destruction performs the required unregister-before-
socket-removal rollback without another cleanup implementation. Socket ownership
moves into the App only in the final infallible assembly step.

The recurring coordinator similarly names the current order without moving
feature decisions out of their owners. Sync and triage still decide internally
whether task data needs refreshing. Logical-day advancement remains part of the
manual refresh path and only rechecks triage when the day rolled. This keeps the
terminal loop structural: tick, draw, poll/read, then one application update.

## Why TUI dependencies use explicit owner paths

The TUI once let modules import the root namespace and inherit names from
wildcard child re-exports. That made local code concise, but it hid which state
owner or integration boundary supplied a dependency. Adding one root export
could also change the usable names in many unrelated modules.

The TUI root now owns only module wiring, the eight-field `App` composition,
and the narrow command-layer entry surface. Production modules import explicit
names from their declaring owner paths. A directory-wide architecture guard
scans production files while recognizing external and inline test modules, and
its synthetic fixtures prove that production wildcard imports and root
re-exports are detected. Focused test modules may still inherit their parent
fixture vocabulary. This makes production dependency direction reviewable
without changing runtime behavior.

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
handlers: a command that reads the wrong default before later honoring `-w`
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
children repeat the canonical `--workspace` selector, and Brain-owned integrations
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
canonical record selected when `--workspace/-w` is absent. Keeping that field at
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
semantics. The agent-controller facade and shared receiver leases are active,
and OpenCode uses the same lifecycle boundary through its adapter and plugin.
Actor context is
attribution and routing, not a new authentication or access-control boundary.

The agent-controller facade keeps frontend-specific command lines, environment
policy, lifecycle hooks, session identity, and PTY control behind one semantic
surface. Claude, Codex, and OpenCode adapters own their syntax differences;
the adapter trait, adapter operation enum, and concrete adapters are
crate-private so callers cannot bypass the facade. Public launch and input
values exist only to let an external transport consume a controller-produced
plan. Callers do not branch on frontend-specific keystrokes. OpenCode's plugin is a
thin event bridge into the existing Brain hooks so attribution, completion, and
delivery continue to use the frontend-neutral state database. The plugin owns
only OpenCode event/SDK translation, root-session filtering, and extraction of
the newest completed assistant text. It deliberately does not authorize DB
rows, rotate lineages, deduplicate completion, publish response files, or route
receiver delivery. Those security-sensitive decisions remain in Brain's
generic Python bridges and one SQLite transaction contract shared by every
frontend.

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
shells, plus the generic session-start bridge (a separate Python process) firing
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

## Why session-start and session-stop bridges have distinct jobs

brain can choose a session id up front (`--session-id`), but if the user
types `/new` (or `/clear`) mid-run, a frontend may rotate to an id Brain never
saw. That fresh conversation is the one they would want to resume next time.
A **session-start bridge** runs for every supported frontend start event with the live
`session_id` (keyed to the shell via `BRAIN_INSTANCE_ID` / `BRAIN_PID` env),
so brain always learns the current id and returns the exact scoped row to
`active`.

The **session-stop bridge** has a separate, per-turn responsibility. It writes the
authenticated completion artifact and marks that same scoped row `completed`,
which lets queued receiver work advance. It does not end the persistent
conversation or make the PTY disposable. The next successful local or queued
submit calls `SessionStore::mark_active`, so ordinary turns after the first one
do not depend on another session-start event to reactivate the row.

Completion authorization, artifact publication, and completion mutation form one
ordered operation. The hook stages a unique synced response file, acquires
`BEGIN IMMEDIATE`, and rechecks the exact currently locked frontend, session,
workspace, actor, channel, and Brain-instance tuple. Its update uses that same
predicate and must affect exactly one row. The response artifact is atomically
published and its directory synced before the database commits `completed`.
If publication or commit fails, the transaction rolls back and the hook
removes or restores only its own published inode. This ordering forbids a
committed completion without its artifact. It also makes a concurrent
session-start rotation win or serialize before completion rechecks the old lineage,
instead of allowing a stale parsed payload to complete an unlocked row.

Rotation authorization and mutation must be one write transaction. A target
ownership `SELECT` followed by a later unconditional upsert leaves a race in
which two shells can both authorize the same free target and the last writer
can overwrite the first. The hook therefore acquires `BEGIN IMMEDIATE` before
reading the exact tuple, source lineage, or target owner. Contenders wait at
the transaction boundary, then re-read current ownership; authorization
no-ops and exceptions explicitly roll back, while the target upsert and prior
session release commit together.

## Why the completion bridge reads a Claude transcript when needed

The final response the user receives over SMS/email exists only if the
session-stop bridge writes the response artifact, and there is no independent backstop
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
input, lifecycle, completion, terminal, and shutdown operations. Claude,
Codex, and OpenCode adapters translate those requests. An exhaustive frontend
registry owns construction, command metadata, lifecycle installation, health
checks, capability evidence, and compatibility probes, so shared callers do
not need another concrete-frontend branch.

The brain panel must control frontend-specific session arguments, so it can't
defer to a shell alias that might inject incompatible flags. Claude remains the
default and uses `claude_cmd` in brain env (default
`claude --dangerously-skip-permissions`); brain appends its own `--resume` /
`--session-id` after that configured base command. A legacy
`brain config claude_cmd` value is honored only when env has no `claude_cmd`,
so existing installs keep working while new edits are machine-local.

Codex is selected per run with `--codex` / `-cx` and uses `codex_cmd` in brain env
(default `codex`) because the right Codex wrapper/model flags can be
machine-specific. OpenCode is selected with `--open-code` / `-oc` and uses
`opencode_cmd` (default `opencode`). `session::build_llm_command` splices each configured base
command in verbatim, then appends the selected frontend's own session shape:
Claude gets `--resume <id>` or `--session-id <id>`; Codex gets `resume <id>`
only when a Codex id is known and no Claude-only flags for fresh launches;
OpenCode gets `--agent brain`, an optional validated `--session <id>`, and an
optional separate `--prompt <text>`.

OpenCode resume evidence comes from `session list --format json` run in the
selected workspace. Brain accepts only live root sessions whose reported
directory resolves to that exact root. This is stricter than trusting a stale
DB row and prevents a matching opaque ID from another root or child session
from becoming a resume target. Missing evidence falls through to the next
candidate or a visible fresh-chat fallback.

The state DB keys sessions by frontend, opaque session ID, workspace, actor,
and channel. Hook upserts, claims, and dead-lock reaping use that exact
composite scope, so equal opaque IDs in different scopes never overwrite or
unlock one another. A separate stable response ID lets the generic completion
bridge signal a fresh Codex or OpenCode turn without pretending Brain chose
the frontend's session ID.

Main-panel launch completes fallible capability resolution and adapter response
identity lookup before claiming a resumable row. Once claimed, only request
assembly and the guarded controller launch remain; a launch failure releases
the instance claim. This keeps a malformed capability configuration or
frontend identity error from removing an otherwise free conversation from the
resume queue, and every failed path clears the response identity for the launch
slot it attempted.

Lifecycle refresh follows workspace singleton acquisition, so a rejected second TUI
cannot alter the lifecycle contract of the live process. Different-workspace
TUIs remain concurrent, and each now updates only its own workspace-local
`.codex/hooks.json`. The adjacent lock and same-directory atomic replacement
still protect two processes targeting the same workspace. The lock prevents
lost read-modify-write updates; the rename prevents readers from observing
partial JSON and preserves the old bytes on failure.

## Why OpenCode support is feature-probed and isolated

OpenCode is supported through the concrete CLI and plugin surfaces Brain uses,
not by assuming every release forever is compatible. Brain probes the required
TUI flags, JSON session listing, pure config resolution, generated capability
schema, and plugin loading. Each probe runs with disposable HOME and XDG roots
so an availability check cannot create or rewrite the user's OpenCode state.
Successful evidence is cached only for the configured command and current
Brain process. This gives users an actionable compatibility error when a future
release changes a required surface while keeping doctor and launch honest.

Inherited `OPENCODE_CONFIG_CONTENT` is merged rather than replaced because it
may contain unrelated user choices. Brain owns only its named agent, default
agent selection, generated `brain_ws_*` MCP namespace, and selected skill-path
addition. Structural type conflicts fail before launch. This narrow ownership
preserves user config while preventing stale Brain-generated MCP entries from
surviving a workspace or capability change.

The PTY still clears the inherited process environment. OpenCode is the narrow
exception: its documented controls share the `OPENCODE_*` namespace, so Brain
copies that namespace into OpenCode launches and then overwrites
`OPENCODE_CONFIG_CONTENT` with the merged reserved layer. This preserves user
configuration paths and frontend controls while keeping Brain's trusted launch
keys authoritative. The namespace is not copied into Claude or Codex.

Registry static artifacts are workspace-owned state, so following an arbitrary
workspace symlink outside the selected root would turn setup into an unrelated
file write. Installation therefore resolves the complete leaf and ancestor
chain before creating directories and rejects any final destination outside the
canonical selected workspace. In-workspace symlinks remain supported and the
atomic replacement targets their confined referent.

## Why workspace capabilities reuse shared frontend authentication

Workspace capability selection is configuration, not a second identity. Brain
therefore keeps portable logical allowlists in the root while commands, URLs,
paths, and credentials stay in that workspace's selected machine record. It
does not create separate frontend auth profiles.

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
remains advisory. OpenCode merges Brain's named agent, generated `brain_ws_*`
MCP entries, and selected skill path into inherited inline config. Brain probes
the generated schema and plugin load but cannot prove inherited global MCP or
skill sources are excluded, so OpenCode also remains advisory. Enforcement
status is derived from concrete launch evidence and never upgraded from
advisory by logical selection alone.

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
configuration from blocking any registered frontend without weakening the mode gate.

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

**Why `brain sync resolve` deletes on the remote too, even though it runs no
sync.** Keep-both writes the loser on *both* sides, and both conflict-name
patterns are bisync excludes — the excludes are what stop a resolved conflict
from being resurrected on the next run, but they also mean a normal sync can
neither delete the remote object nor bring it down for the user to see. A
local-only resolve therefore looked clean while leaking one orphan object into
the bucket per conflict, forever, visible to no brain command. So resolve owns
both halves: it is still *deletion only* (no bisync, no journal entry, no
`--resync`), just deletion in both places. The two halves are deliberately
independent — a resolve whose local copy is already gone still collects the
remote orphan — because that is the state every conflict resolved before this
lane existed is already in, and the state a second machine leaves behind. The
lane matches losers by *both* naming forms (the remote keeps rclone's raw
marker, since only the local root is ever renamed), lists only the original's
own directory rather than the whole bucket, and uses `deletefile` rather than
`delete` so a bug can never recurse over a directory. A remote that cannot be
listed reports "could not check the remote" instead of the reassuring silence a
clean remote would produce: with a destructive lane, the failure mode that
matters is a false claim of success.

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

**Why habit occurrences dedup by `(task_name, due_date)` instead of staying
UUID-distinct.** The rule directly above — UUID-distinct rows both survive —
is correct for genuinely independent rows, but a recurring habit's next
occurrence breaks that assumption: it's a *new* row (a fresh `task_uuid`,
minted by `spawn_next_occurrence`/`reconcile_enabled`) representing something
that isn't actually new — the next date of the same recurring commitment. If
that occurrence gets spawned on two machines before they sync (complete the
same habit on your phone and your laptop before either syncs, say), the two
spawns are UUID-distinct by construction, so the id-keyed merge has no way to
recognize them as the same occurrence and both survive as ordinary "added"
rows. That produced real duplicate habit rows in practice. Two options were
considered: (a) prevent the race at spawn time (e.g. a deterministic UUID
derived from `task_name` + `due_date`, so both machines would mint the *same*
UUID and the id-keyed merge would collapse them for free), or (b) detect and
collapse the duplicate after the fact. (a) was rejected: it would change the
UUID scheme's meaning everywhere else (every other row's UUID is opaque
identity, never derived from mutable content) for a benefit narrow to one
race window, and a content-derived UUID stops being stable the moment
`task_name` is edited. (b) — a dedup pass keyed on `(task_name, due_date)`,
scoped to habit-shaped tables only, running after the row union and before
display-ID reconciliation — fixes the actual failure mode without touching
UUID semantics anywhere else. Folding duplicates through the existing
`field_merge` rules (completion wins first, then last-touched) rather than
picking one wholesale means a `done` duplicate is never silently discarded in
favor of a `not_started` one, whichever side produced which. The survivor's
UUID is the lexicographically smallest in the group — the same
side-independent tiebreak used for display-ID collisions above — so this
stays convergent and idempotent by construction: a table with no duplicate
occurrences, or an already-deduped one, is unaffected.

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

## C4: live-shell auto-sync (`notify`, periodic pulls, freshness gates, and the sync lock)

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

**Why there is a fixed five-minute pull but no exit sync.** Field use showed
that startup and receiver freshness alone leave a long-running main TUI stale
after another machine publishes a change. Each configured live shell therefore
starts a fixed five-minute pull timer. It is intentionally not another config
knob: the invariant is that an open Brain converges promptly. The detached
runner and UUID lock make duplicate timers cheap to coalesce. Exit sync remains
redundant once local writes and receiver completions push, and it complicates
shutdown. The receiver's two-hour value remains a message-time freshness
threshold, not its polling interval.

**Why receiver completion pushes explicitly.** The filesystem watcher is the
general local-write safety net, but receiver work has a precise durable
completion boundary. Launching a push there prevents an agent-created task from
waiting for watcher delivery and still uses the same detached runner and lock.
The common receiver prompt also disambiguates task-capture wording from a
request to perform the task immediately.

**Why read-only check compares size and checksum.** Filesystem and remote mtimes
can drift even when bytes are identical, which made `brain check` report
phantom changes such as task metadata. Its dry run adds
`--compare size,checksum`, which ignores timestamp-only drift while still
detecting same-size content changes. Real sync retains rclone's default
mtime-aware comparison because its newer-side conflict resolution needs that
ordering signal.

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

brain is meant to be cloned and used by anyone, and its skills must be available
to every supported agent frontend opened in the selected brain root. Embedding
the `skills/` dir into the
binary with `include_dir` makes it self-contained: `brain skills sync` writes the
skills out wherever they're needed, so a user who `cargo install`s brain (or
moves the binary) still gets them. `include_str!` can't carry a skill's multiple
files (SKILL.md + scripts), which is why the one dependency is justified.

## Why bundled skills are project-scoped

`brain skills sync` writes rendered skills directly to
`<brain-root>/.agents/skills/<name>`, then links each project frontend's
`.claude/skills`, `.codex/skills`, and `.opencode/skills` entry to that copy.
Brain no longer writes `~/.agents/skills`, `~/.claude/skills`, `~/.codex/skills`,
or any other machine-global frontend registry. This keeps skills aligned with
the selected workspace and prevents one brain workspace from changing another
project's agent behavior. The link targets are a pure function
(`layout::link_ops`), unit-tested; the filesystem shell (`install`) stays thin.
The sync also discovers valid user-authored skill directories already under
`.agents/skills` and links them without rewriting their contents. A new-version
migration pass renders the embedded core set into every registered workspace,
detecting legacy global locations for observability while leaving those old
files untouched. TUI startup excludes its selected root from that pass because
the normal startup sync handles it immediately afterward.

## Why skill auto-sync had a rollout gate (historical; default now on)

During sub-projects B1 through B3, `resync_skills()` was gated off because the
live registry still had another owner and the render/install pipeline was not
ready. The B4 cutover completed that ownership transition and activated the
same seam.

`skills_auto_sync` now defaults to `true`: config/personalize mutations and the
first ready-workspace invocation after a version change render the selected
workspace's `.agents/skills` directory. Setting it `false` leaves only explicit
`brain skills sync`.

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
*built* copy that registered frontends read (`render` → `install`), leaving the source
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
The daily-triage tab used to close the instant the run POSTed its
one-time token, and in practice the model fired that POST as soon as the *task*
passes finished — before an extension's printable/PDF was baked — so the tab
died mid-bake and the output never landed. The tempting fix (have the code wait
for the PDF) is exactly wrong here: the agenda, the markdown, and `~/Downloads`
are all a *user extension's* concern (`triage:daily-merge`), and the core skill +
`skill_session/signal.rs` must assume **nothing** about whether any such extension
exists or what files it writes. So the fix is a generic contract: the completion
POST carries a `require` list of output paths *the run itself declared* (an
extension supplies them at `triage:daily-required-outputs`; core supplies none),
and `App::tick_skill_sessions` holds the signal and refuses to close until every
listed path exists (`skill_session::signal::ready_to_close`, pure). An empty list —
the no-extension / fork case — closes immediately, identical to the old
behavior. This is the reference case for the extension-agnostic rule now written
into [AGENTS.md](../AGENTS.md): skill-related code and core skill text may assume
a hook *might* carry extension content, never what it contains, and every
generic mechanism must no-op when no extension contributes.
Keeping personal tokens out of a bundled skill is a **review step, not an
automated test** — see "Why there is no automated personal-data guard test"
below.

Cross-skill script calls (todo's `find_chronic_ignored.py`, …) use the selected
workspace path `$BRAIN_ROOT/.agents/skills/todo/scripts/<name>.py`. This keeps
the bundled skills frontend-agnostic while ensuring each brain root carries its
own complete skill set.

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

## B4 — historical shared-registry cutover (superseded)

The B1–B3 pipeline was proven only in a sandbox; B4 was the phase that was
allowed to touch the live agent registry. The cutover flipped the six migrated skills
(`article-summarizer`, `brain-knowledge-capture`, `contacts`, `second-brain`,
`todo`, `triage`) plus two plugins (`zotero-sync`, `linear-sync`) from
dotfiles-manager-owned to brain-owned, and makes the dotfiles manager delegate to
`brain skills sync` without ever pruning what brain owns.

This section describes the historical shape of the cutover for anyone whose skills
are currently owned by a symlink-based dotfiles manager. Brain itself knows
nothing about any such tool; all the coordination is on the dotfiles-manager
side.

**Historical ownership boundary.** A registry
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
(startup, periodic pull, watcher, receiver freshness, receiver completion)
spawns a detached
`brain --workspace <canonical-name> sync --if-idle` child (`process_group(0)` +
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
task tables. The modal then appears only if triage is genuinely still due; if
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
If another captive overlay owns the shell's exclusive modal slot when refresh
finishes, the gate keeps the refreshed alert decision pending. It does not
repeat the sync refresh; after the user dismisses that overlay, the next event
loop tick displays the triage nudge. An already-visible triage confirmation is
still reconciled immediately and withdrawn when the refreshed habits prove it
stale.

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

## Why every HTTP route resolves an exact workspace before workspace state

Names, roots, defaults, and query parameters are mutable selectors and cannot
safely identify a workspace at a machine-wide listener. Provider endpoints are
the two machine-wide paths `/sms` and `/email` and resolve their workspace from
the destination the provider signed (see the next decision). Local habits and
triage actions use `/local/<exact-live-lease>/w/<opaque-ingress>/...`, so a
whole-port provider tunnel cannot publish local reads or mutations. The pure
router parses these capabilities before any handler runs.
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
exist before state publication, closing the startup signal window. Accepted
receiver work lives in each workspace DB, never in machine-shared process
state. The design deliberately has no process-owned replay worker, headless
agent, manual restart, or always-on responder.

## Why status probes bypass ordinary bootstrap and logging

The shared lifecycle acceptance gate treats status as observation, not as an
opportunity to make the selected workspace ready. Ordinary ready-workspace
bootstrap may migrate the registry, initialize access config, recover portable
user transactions, refresh installed skills after a version change, and write
the render stamp. The ordinary run logger also creates a private `/tmp` file.
Those are correct for commands that will work with the workspace, but they
make a status probe mutate the thing it is measuring.

`brain server status` and `brain receiver status -w <workspace>` therefore pass
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
open the exact workspace's UUID-scoped durable queue. The shared process does
not start for inbound traffic and contains no queue consumer, replay worker,
headless agent, or availability-only responder.

The shared fixed four-worker boundary covers habits, triage, SMS, and email.
Receiver bodies and serialized job frames are limited to 1 MiB, and each
workspace accepts at most 64 durable `queued` rows. Progressed, retrying,
failed, and done rows do not consume queued ingress capacity. Final registry
and exact-revision checks authorize an in-flight admission; only its atomic
commit allows the server to enter SQLite acceptance. Disable and unregister cancel pending
or authorized admissions. If commit already won, revocation waits outside the
control-state mutex only until the original request deadline. A timeout rejects
the control request and applies no later lease mutation. Watchdog expiry removes
the exact lease first, preventing new admissions, then cancels every matching
pre-commit admission. Ordinary lease operations filter expiry but never remove
it; shared control and watchdog entry use that single revoke-aware removal.
Final admission performs persisted-intent filesystem IO outside the control
mutex. One combined commit operation then acquires control, samples exact TTL,
revalidates the route and admission identity, and performs the admission CAS
before unlocking. The complete job/conversation insert, durable provider
deduplication, and queued-capacity decision then share one immediate SQLite
transaction. Disabled, missing, full, and failed-storage endpoints receive one
channel-specific unavailable response and create no new row.

The cap intentionally counts only `queued` rows. This preserves the old
64-entry waiting-queue bound, where one active job had already left the
`VecDeque`; progressed work is execution evidence rather than waiting ingress.
Provider and exact-job deduplication run first so a response-loss retry still
resolves to the original row when all 64 waiting slots are occupied. An
immediate transaction serializes the count and insert, preventing concurrent
requests from overbooking the final slot.

The pre-BR-13 TUI socket path remains characterized for the BR-14 executor
cutover, but it is no longer provider ingress acceptance. The TUI keeps its listener nonblocking so each event-loop poll stays bounded,
but explicitly returns every accepted job stream to blocking mode before
applying fixed read and write timeouts. Some platforms can otherwise surface
`WouldBlock` while a sender is still completing a frame, turning a healthy live
TUI into an intermittent unavailable response. Bounded deadline polling in the
integration suite exercises this boundary without fixed sleeps.

The live queue is represented by `InboundQueue`, not by a collection exposed
through `App`. It alone owns the 64-entry `VecDeque` and every mutation:
admission staging/finalization/rollback, successful FIFO head commit, and the
two control-command cuts. Keeping those decisions together prevents socket,
dispatch, completion, and control callers from depending on representation or
silently changing FIFO semantics. Tests receive only an owned read-only
snapshot.

The architecture guard enforces the ownership boundary structurally: outside
`receiver/queue.rs`, no source-declared persistent TUI item type, initializer,
or resolved import/type alias may mention `InboundJob`. A dev-only `syn` AST
walk covers complete struct, enum, union, type-alias, const, and static items
in module, associated, foreign, function-local, and arbitrarily nested block
scopes. Import and type aliases resolve in their lexical item scope, including
associated aliases, and every identifier comparison canonicalizes raw Rust
identifiers with `IdentExt::unraw`. The guard scans source files independently,
so the declared-item/export invariant also rejects every visible renamed
re-export of a resolved `InboundJob` alias outside `receiver/queue.rs`. This
makes a cross-module rename fail at its declaration even though a sibling's
plain-name import cannot be linked across separate ASTs. Private same-scope
renamed imports remain resolved and valid. The guard does not depend on field,
collection, alias, or mutation names. Item macros and all opaque `Verbatim`
item forms fail closed. Statement
macros remain valid only when recursive `proc-macro2` token-tree inspection
finds no raw or resolved job alias. Non-builtin attributes on persistent items
are rejected because they could generate storage outside the declared AST
surface. Local transient values and calls through the semantic `InboundQueue`
API remain valid. `cfg(test)`, `cfg(all(test, ...))`, and only other conditions
that logically imply `test` are excluded; mixed production conditions are
scanned conservatively.

The sole exception is the top-level `ReceiverEffect` item at the exact
manifest-relative `src/tui/receiver/effect.rs` path, and only its named direct
one-shot payload shapes using canonical `std::boxed::Box` and
`crate::server::receiver` paths. A generic, nested, or same-named item,
suffix-matching path, shadowable import, collection, tuple, or different
payload variant receives no exception. `syn` parses source but does not perform
procedural expansion, and a type-erased runtime value does not expose its
concrete contents in a declared item type. Those two cases remain explicit
manual-review limitations. Blocking opaque item macros and unsupported
attribute macros prevents them from silently bypassing the source-declared
ownership surface.

The original architecture spec and implementation plan said no dependency
would be added. The implemented guard accepts a narrower rule: no new shipped
or runtime dependency. Direct dev-only `syn` and `proc-macro2` declarations are
intentional because both crates were already transitive dependencies and they
replace the unsound handwritten source parser. Making them direct test
dependencies keeps the AST contract explicit without changing the binary.

This staged-socket rule describes the superseded ingress boundary and remains a
runtime characterization until BR-14 removes or adopts it. If the TUI stages after `commit` but cannot write its final `accepted`
acknowledgment, an opaque admission token bound to its issuing queue identity
and admission generation removes only that exact staged tail item before
releasing the exclusive queue borrow. A successful write finalizes the same
token and makes the job dispatchable. The old server path therefore treated the
failed handoff as unavailable and never committed an ID for work the TUI did
not acknowledge. BR-13 instead commits the durable job before provider success
and does not append through this socket.

## Why receiver conversations keep both native history and a Brain transcript

The durable conversation is not a rule to start a new agent session for every
message. One logical receiver conversation owns its current frontend and
native session ID, so normal continuity is a same-frontend native resume. The
identity is specific to workspace, portable user, channel, and channel lineage;
there is no machine-global Email session or SMS session. SMS has one stable
lineage for that tuple. Email reuses only a verified provider thread and never
guesses from its subject.

Resend's authenticated payload currently exposes a per-message ID rather than
a verified stable thread key. Each new Email delivery therefore uses
`EmailLineage::Uncertain`, even when subject or message fields match. A retry of
the same provider delivery resolves to the original durable job and
conversation before the fresh identity can create another conversation.

Native history alone is insufficient durable authority. Its storage and resume
rules belong to Claude, Codex, or OpenCode, it may be deleted independently of
Brain, and another frontend cannot safely resume it. Email reply quotes are not
a substitute either: a sender may delete them, providers can truncate them,
and they mix presentation with conversation state. Brain therefore maintains a
portable markdown transcript alongside the native binding. If the requested
frontend changes or its native evidence is unavailable, the next session starts
fresh from that transcript. The transcript is recovery input, not a reason to
discard a healthy same-frontend session.

The same durability principle applies to jobs. A claim records expiring owner
authority on the row instead of popping it. On crash, queued work becomes
claimed. Due-retry work keeps `retrying` while its consumed schedule clears, so
the new live owner can resume either launching or delivering. Work already
launching, accepted, processing, answer-ready, or delivering keeps that
progressed state as its lease changes owners. Erasing those states to claimed
would prevent the later recovery policy from knowing whether a same-session
recovery attempt is appropriate. Failed and done remain terminal.

BR-12 intentionally stopped at this model boundary. BR-13 now uses it for
provider ingress, including durable deduplication and queued capacity, but does
not decide when a live TUI should inject input or replace agent execution,
completion, or delivery. BR-14 and later PROJ-1 tasks own those consumers.

## Why the receiver tick uses ordered decisions and effects, not one lifecycle enum

A receiver tick observes several independent dimensions: an interactive turn,
a remote completion, a processing-response delay, a panel activity sample, an
activity probe, a turn timeout, a warm-session lease, a retry deadline, a sync
freshness gate, control messages, and queued work. Combining those dimensions
into one lifecycle enum would create a large cross-product whose variants
encode incidental timing combinations rather than real domain states.

`receiver::decision` therefore keeps those inputs as independent `TickFacts`.
`TickStage` names only the historical execution order, and each pure stage
decision produces at most one typed `ReceiverEffectKind`. The runtime
re-snapshots facts before every stage and materializes one-shot targets only
when that decision is reached. The App executes the corresponding
`ReceiverEffect`, retaining ownership of `AgentController`, response files,
provider delivery, task reloads, child processes, and sync observations. It
then feeds semantic completion, dispatch, diagnostic, or freshness results
back to the runtime. Re-snapshotting prevents duplicate state transitions and
allows an earlier effect, such as a timeout or `/restart`, to change all later
decisions in the same tick.

The receiver facade owns `receiver::policy`, whose pure timeout,
activity-probe, retry, and input-lock decisions support that runtime. Keeping
the policy below `receiver/mod.rs` makes the facade's ownership explicit and
does not introduce a second receiver-state owner.

The fixed order remains remote completion, interactive completion, processing
delay, panel activity, activity probe, turn timeout, warm-lease expiry, socket
polling, `/restart`, retry readiness, sync freshness, `/new`, idle-panel
selection, and dispatch. `/new` may repeat within its own stage to consume
consecutive control messages, and a waiting retry or sync gate halts the
remaining stages. The production effect executor reports semantic outcomes and
the pure coordinator maps those outcomes to advance, stop, or repeat-current-
stage control. Consequently App neither interprets a raw sync boolean nor
special-cases an effect variant to choose control flow. `/restart` completes
normally: later stages re-snapshot the real queue and can dispatch a job that
survived the restart cut in the same tick. This is sequencing policy, not a
second mutable receiver state machine.

Webhook verification follows provider replay guidance: HMAC comparisons are
constant-time and Resend timestamps have a five-minute tolerance. Provider
delivery IDs are durable keys scoped by workspace and channel. Every
nonconcurrent retry reaches SQLite acceptance, where provider deduplication
precedes queued-capacity rejection. Process memory excludes only an in-flight
duplicate, which is unavailable rather than prematurely acknowledged. A known unavailable Resend ingress is
still resolved before credentials; only that routed workspace's signing secret
is then loaded to verify the event. A verified unavailable Resend ID is retained
as a permanent discard, so later TUI availability cannot replay it. When the
same ID is already in flight, the verified path records a deferred discard,
leaves the reservation owned by the pending acceptance, and returns 503. The
pending completion promotes the deferred discard before a later retry. This
1024-entry set is bounded discard memory, not durable-success authority, a
queue, a replay worker, or a headless path.
Persisted disable remains authoritative before live refresh: its failed route
retains exact ingress-to-workspace identity for the same verified discard.

The accepting request captures immutable actor, channel, normalized sender,
response email, and allowed authenticated-thread recipients. The TUI routes
that same context through `AgentController` for Claude, Codex, and OpenCode. Configuration
changes during the turn cannot replace the initiating actor or broaden reply
recipients.

The route ticket remains attached to that accepted context. After provider
work and actor/job construction, dispatch reloads the exact canonical registry
record and requires its immutable workspace UUID and persistent receiver intent
to remain valid. It then reacquires the control mutex only to revalidate the
exact generation, authority revision, receiver enablement, and live lease, and
releases the mutex before durable workspace admission. At admission commit,
persisted intent is reloaded outside the mutex; one combined operation
then locks control, samples exact TTL, revalidates that same authority and the
admission's workspace/lease identity, and performs the admission CAS before
unlock. The attached
authority revision and cancellable admission reject notified,
notification-lost, unregister, and disable-enable ABA revocation without
holding the mutex during provider or database work.

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
durable admission at two seconds. One absolute handoff deadline is installed
as SQLite's busy timeout before WAL configuration or schema reconciliation,
then freshly recomputed and rebound after open before acceptance lock waiting.
The deadline is
checked after commit, so successful progress cannot consume a renewed timeout.
One shared compile-time timing
invariant prevents these bounds from drifting apart. The curl reader stops
after one over-limit proof byte and reaps the child before returning a typed
502. Resend receives HTTP success only for verified unavailable, ignored, and
permanent discard outcomes so discarded webhooks cannot be replayed into a
later live TUI. An exact in-flight unavailable duplicate receives 503 until
its deferred discard is promoted; signature failures remain authentication
failures, while 500 and 502 remain provider-visible failures. Accepted email jobs
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

Native receiver continuity fails closed because a stored opaque binding is a
hint, not proof that frontend history still exists or is available to this
run. The BR-14 launch planner therefore requires the binding's frontend to
match, validates it through `AgentController`, and accepts it only after the
caller's exact-session claim succeeds. Missing, corrupt, incompatible, or
unclaimable history starts fresh from Brain's portable transcript. That
recovery prompt is capped at 64 KiB and preserves the newest UTF-8-safe
transcript suffix because recent turns are the most useful recovery context;
the current authenticated message and attachment references remain in a
separate section. Resume omits the transcript entirely. The plan contains no
tab, durable claim, or binding mutation, which keeps frontend translation
behind `AgentController`; the durable coordinator owns those effects without
widening the planner.

Receiver launch ownership is isolated from the interactive shell. Every remote
run gets a unique instance ID and either claims the exact validated resume
session or registers a Brain-supplied fresh ID before process launch. The
existing lifecycle bridge is the authority: Claude may confirm that registered
ID as its native session, while Codex and OpenCode must rotate it to a distinct
native ID. Brain rejects an unproved placeholder and performs a binding-only
conversation update so portable transcript history cannot be lost. Binding
requires the complete durable workspace, logical conversation, frontend,
actor, channel, remote instance, and registered-ID tuple; the lifecycle-reported
actual ID is retained alongside it. An armed
registration guard releases the exact remote owner on early return without
touching the main instance. Rollback invokes that cleanup explicitly so a
release failure is reportable, while `Drop` remains a best-effort fallback.

Launch retries stop at the pre-acceptance boundary. The exact live owner alone
may move `claimed`, or a due retry originating in `claimed`/`launching`, to
`launching`. Planning, registration, tab allocation, and spawn failures stop
the controller, release the remote owner, and durably schedule at most two more
attempts using a stable content-free reason; all rollback steps run even when
controller shutdown or exact-session cleanup reports a diagnostic, and the
concrete shutdown diagnostic remains available to its caller. The third
failure marks the job failed without deleting it. Reclaimed `accepted`,
`processing`, answer, and delivery work stays unlaunched until BR-16 defines
its recovery policy.

**Why one durable consumer and no main-panel reuse.** Receiver work must not
compete with a second in-memory execution cursor or inherit interactive panel
state. One recurring App tick therefore owns durable FIFO claim through
terminal close. Every frontend receives a new `AgentController` and PTY under a
unique remote instance, even when the main panel is hidden or idle. Background
tab insertion and removal preserve the user's current view and focus, and no
receiver path types into or submits through the main panel.

**Why freshness and ownership precede progress.** A claimed job is renewed
before a pending freshness pull so a slow sync cannot let another owner launch
the same work. While one receiver tab is active, later arrivals stay durable and
unclaimed. Losing exact ownership permits local controller and tab cleanup only;
mutating the job, session, lifecycle, or reply would race the new owner.

**Why terminal completion requires exact lifecycle evidence.** Process spawn
and screen activity do not prove acceptance or completion. Brain requires the
exact completion artifact and exact locked remote session for the launched run.
A valid completion currently moves `launching` directly to `done`, because
BR-15 owns accepted and processing proof. Child exit without that evidence is a
pre-acceptance retry. Reclaimed progressed states remain unchanged until BR-16
defines their phase-specific recovery.

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

## Skill sessions: N ephemeral single-prompt tabs + a per-token flag-file bridge

Daily triage can be long and interactive, so running it inline in the main brain
session blocked everything else until it finished. We moved it into its own
brain-panel **tab** so the pass runs as a background task — and then generalized
that one tab into **skill sessions**, because daily triage is not the only long
prompt a user wants out of their main session (a personal `/email-triage` pass is
the motivating second case).

**Why a general list now, when a dedicated `triage_brain` slot was the earlier
decision.** The original constraint was that a user must not be able to spawn
*arbitrary* sessions, which a dedicated `Option<AgentController>` plus a
two-variant `BrainTab` modelled exactly. What changed is not that constraint but
where it is enforced: sessions are still not arbitrary — each one must be a
**declared definition**, either the builtin daily triage or an entry in the
workspace's `skill_sessions` env array, and a definition already running offers no
way to start again. So the set of possible sessions stays finite, named, and
inspectable, while `BrainPanelState` allows the several *different* long runs a
user really does want in parallel. Receiver and session state stay centered on
the same aggregate's dedicated main controller exactly as before.

**Why definitions are env, not portable config.** A skill session names a prompt
whose skill must actually be installed on *this* machine (`/email-triage` is a
personal global skill, not a brain-bundled one). A definition that travelled to a
machine where the skill is absent would offer a palette row that fails, so the
list is machine-local brain env, alongside the other "what can this machine
actually run" values.

**Why a tab identity rather than a list index or a definition key.** A tab is
addressed by a monotonic `SessionTabId`. An index would let closing one tab
silently repoint the active tab at another; a `SkillSessionKey` would break if the
user edited `skill_sessions` while a session was running. Receiver runs also
need stable identity without pretending their durable job is a configured
skill. One checked counter therefore spans both distinct metadata variants and
never reuses an ID. The rendered strip, `Alt+<digit>` slots, and the `Alt+[` /
`Alt+]` cycle all consume that same insertion order.

**Why one collection with distinct metadata.** Controller ownership,
allocation failure cleanup, title ordering, active-controller lookup, and shell
shutdown are identical for skill and receiver tabs. Duplicating those mechanics
would create two orders and two cleanup paths. Their lifecycle facts are not
identical, so the collection stores a kind enum: skill metadata owns its
definition key and completion token, while receiver metadata owns its durable
job and remote instance identities. Receiver insertion only mutates this
collection. It never invokes the shell selection path, and receiver-only state
does not reveal a hidden panel. The durable receiver coordinator uses that
narrow insertion and removal surface, so background work cannot select a tab or
change the main view, panel visibility, or keyboard focus.

**Why the session is untracked.** A skill-session tab is ephemeral by
construction. `App::open_skill_session` builds an `AgentController` from a
`LaunchRequest` whose hook metadata carries only `BRAIN_SESSION_DONE_URL` and
`BRAIN_SESSION_TOKEN`. The adapter adds the common workspace identity and agent
kind, but the request has no instance ID, state DB, or response ID. The
session-start bridge therefore never records it, and it is never a resume
candidate. If the shell closes mid-run the session is lost and the user simply
starts it again (and the startup nudge fires again next launch for daily triage),
which is the desired behavior — resuming a half-finished single-prompt run is not
something a user can reason about.

**Why the completion protocol is appended to the prompt.** Daily triage could
carry its POST instruction inside the bundled `/triage` skill because brain owns
that text. brain owns none of a user's own skills, so the protocol travels with
the prompt instead: `skill_session::prompt::launch_prompt` appends the POST
instruction, the env var names, and the `require` convention to whatever prompt was
configured. Any skill therefore participates unmodified, and there is exactly one
protocol rather than one per skill.

**Why a completion signal instead of idle-detection, and why via the brain
server.** "The agent went idle" is unreliable because these passes ask the user
questions. The run therefore POSTs an explicit completion signal (with a one-time
token) once it truly ends. It targets the shared process already attached to the
live TUI; opening a tab never elects or starts a server independently. A localhost
`POST /local/<exact-live-lease>/w/<selected-ingress>/session/done` carries the
exact live TUI's capability and matches the local habits-completion precedent.
Because the server is a *separate process* from the TUI, the signal crosses on
disk and the matching TUI polls it in its existing per-tick loop, the same
poll-of-disk pattern the triage nudge and receiver responses already use.

**Why one signal file per token.** With several sessions open, a single
`triage-done.json` would let whichever run finished first close whatever tab was
listening. Signals are therefore `<workspace-cache>/skill-sessions/<token>.json`
and each tab reads only its own. Since the token names a file and arrives in an
HTTP body, `signal::parse_signal` rejects anything that isn't a safe file name
before it can reach the file system, and the shell clears the whole directory at
startup so a signal orphaned by a crashed run can't close a later tab.

## Palette commands carry a per-command `is_visible` predicate

Command-palette visibility used to be a single growing `match` in
the task-palette catalog builder that special-cased each conditional command inline
(`CloseBrain` needs a panel, the receiver rows need a running/stopped server,
the notes/links rows need notes/links). Adding the brain-panel tab-switch rows
would have meant extending that match yet again.

Instead each `PaletteCommand` now carries an `is_visible: fn(&TaskPalette) ->
bool` predicate (default `always`). The catalog builder applies the *structural*
gate (`command_in_scope`: task-vs-global, the habit filter, the logs-view whitelist,
the task-actions-modal restriction) and then the command's own predicate. The
conditional logic lives next to the command it governs, new conditional commands
are a one-line predicate, and `TaskPalette` is the single snapshot of TUI state
the predicates read, seeded at open time from the relevant `App` fields.

**Why the tab-switch commands exist at all.** `Alt+1` / `Alt+<n>` are the intended
tab switches, but terminal `Alt+digit` handling is unreliable — many terminals
can't distinguish `Alt+1` from a bare `1`, and the encoding varies by terminal
and keyboard layout. In a TUI where a focused brain panel forwards every key to
the child agent, the *reliable* app-level surface is the command palette
(`Ctrl+P` → filter → Enter), so **Show main brain session** and the per-tab
**Show \<title\> session** rows are the works-anywhere path; the Alt chords remain
as a bonus where the terminal supports them.

**Why palette rows became owned values.** The row list used to be
`Vec<&'static PaletteCommand>` straight off the const table. A workspace's skill
sessions contribute rows whose labels come from its own env, which cannot be
`&'static`, so `TaskPalette` now builds owned shared `PaletteRow<TaskAction>`
values (number, label, action, shortcut) and splices the skill-session rows into
the brain-tab group. The const table still fixes the order of everything brain declares itself,
and the start rows are omitted for sessions already running — the same pure
`skill_session::runnable` decision that the tab list is derived from, so a row can
never disagree with the tabs that exist.

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
failed with `brain user local <USER_ID> -w <name>`. That is right when the
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
only fires when the feature is on — is still dismissed). And it selects the row
by stable `system_key` rather than by id, mirroring the bundled
`apply_sync_rules.py --complete-managed-triage`, so no caller has to know which
occurrence id the current cycle happens to carry.

## Why "managed" protects deletion but not completion

`ManagedTaskError` originally had a `ManagedTaskCannotComplete` arm, so the
ordinary complete paths (`brain tasks complete`, the TUI's mark-complete, the
habits page's done button) refused a managed triage row and pointed at
`complete_managed` instead. That was the wrong line to draw. What Brain owns
about a managed chain is its **existence and cadence**: reconciliation
guarantees exactly one open occurrence per enabled chain, which is why removing,
reviving, and skipping one are still refused (each leaves reconciliation with
nothing coherent to maintain, or silently manufactures a second pending row).
Completion threatens none of that — `complete_habit` marks the occurrence done
and spawns the next from a clone that keeps the `system_key`, which is precisely
what `complete_managed` does. The user doing their own triage and ticking it off
in the browser was being told "managed triage habits cannot be completed
outside triage", which is both wrong and unfixable from where they stood.

So completion is now unguarded everywhere and `complete_managed` is no longer an
exception to a protection — it is just the id-free, `system_key`-keyed entry
point that the nudge's Skip button and the CLI share. The daily-triage nudge
already asks "does any occurrence of the named habit carry today's
`completed_date`?", so a manual completion suppresses the modal with no extra
wiring.

## Why a superseding lease inherits the capability it replaced

`brain habits` elects a background process, attaches a browser-only lease
(`tui_pid == 0`), and hands the browser a URL containing that lease's ID as its
local capability. Starting a TUI for the same workspace replaces that lease with
a real one — and the already-open page cannot know it, so every later request
carried a capability the table no longer recognized and the page died with
`local route not found`. The user's only recourse was to relaunch habits from
the TUI to get a fresh URL.

The lease table therefore records the one capability a superseding lease took
over (`inherited_capabilities`), and `begin_local` accepts either the live
lease's own ID or that inherited one. It is deliberately narrow: one capability
per workspace, only from the background-to-TUI takeover, retired the moment the
holding lease unregisters or expires. Nothing else about the route relaxes —
the ticket still resolves to the *live* lease, so authority revision, generation,
and post-IO revalidation are unchanged, and a capability that never owned the
ingress is still a 404. The alternative (having the TUI adopt the background
lease's ID) would have entangled registration, heartbeat, and unregister
identity for a purely presentational URL.
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
workspace-sensitive data or the state DB is opened, closing the notification
race. Status deliberately prints intent, TUI liveness, process reachability,
and effective acceptance separately. This preserves TUI-only execution while
durable ingress remains workspace-scoped and the shared process gains no queue
consumer, headless agent, manual lifecycle, or always-on responder.

## Why receiver setup joins machine credentials to portable users by workspace

Provider credentials and public routing origins describe one machine's
connection to one workspace, so setup writes them only into the already
selected schema-v2 machine record. Inbound identity describes a portable
person, so the corresponding phone or email belongs in that workspace's
`users.json`, not in process state or a machine-global allowlist. The setup
planner requires only the address family selected by the channel and carries
an explicit inbound-allowed value for headless parity.

The portable manifest remains the sole owner of ingress identity, which now
serves local capability URLs and lease routing rather than any provider URL.
Only new workspace initialization creates an ingress; attach, rename, alias, and
default changes preserve it. Provider URLs are stable for a different reason:
there is one per channel for the machine, and it is derived from the
machine-global public origin alone.

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

## Why identity mapping adopts an existing person instead of only creating one

A legacy workspace assigns work to whatever string its owner typed: `me`, a
first name, an ID that was retired years ago. The rollout's mapping gate used to
treat every unresolved `assigned_to` value as a new person and ask only for a
display name, so the one answer a human most often wants — "that is me, and I am
already in this registry" — could not be given. The result was a duplicate
person for the same human, which the owner then deleted by hand, leaving the
task rows pointing at nobody.

So a mapping answer now chooses between the people who exist and adding someone
new, and adopting an existing person is a first-class outcome. Because the
portable model stores the member ID directly in `assigned_to`, adoption cannot
be recorded in `users.json` alone: it is an `AssignmentRewrites` entry that the
journaled task-schema cutover applies to both CSVs, inside the same transaction
that establishes UUID merge identity, with the retained backup holding the
pre-rewrite values. That is also why the rewrite happens there rather than in an
earlier step: the durable backup is captured before the cutover, and a second
writer between the two would make the backup and the migrated files disagree.

Adoption also removes the old requirement that a legacy assignment value parse
as a portable user ID. `Wife` could previously stall the whole rollout; it can
now be adopted onto `sam` and disappears from the data. Keeping a value as a new
ID still requires exact lower-case kebab case, so the registry never gains a
malformed ID by accident.

The same decision is available outside the rollout as `brain user reassign`,
which is both the headless equivalent of the interactive answer and the repair
for a workspace that already migrated with a duplicate person. It never mutates
`users.json`, so a mistaken value cannot silently create or delete a member; it
reports how many rows moved and writes nothing when none do.

## C6 — habits are excluded from every cleanup pass, enforced at the writer

A habit's pending row *is* the habit. `habits.csv` holds no separate definition
of a recurring habit — only its occurrences — so the single `not_started` row is
the only record of the cadence, the priority, and the `ideal_time`. Completion
appends the next occurrence and the chain walks forward one row at a time.

That makes a habit row look exactly like a stale task to a cleanup pass, and
catastrophic to treat as one: dropping a past-due habit row deletes the whole
chain, silently taking every future occurrence with it. A past-due habit is not
deadwood — it is the normal resting state of a habit the user hasn't gotten to
yet, and being weeks late carries no signal that they want it gone.

This bit us for real: a triage pass dropped 21 live `not_started` habit rows
(daily and weekly chains the user still kept), and because each row was the
chain, the habits simply stopped existing. Nothing errored and nothing was
reported, because deleting a habit row is a perfectly legal CSV write.

So the exclusion is enforced at the writer rather than only asked for in prose.
`remove_task.py` resolves needles through `_csvlib.locate`, which searches
`tasks.csv` *and* `habits.csv`; it now refuses a habit row unless the caller
passes `--habit`, and refuses `--habit` for a task. The bundled `triage` skill
is forbidden from ever passing that flag, so the destructive path is unreachable
from a cleanup pass even if the model misjudges a row. `backlog_task.py` already
refused habits; `cleanup_done_habits.py` only ever removes `status=done` rows
past their 7-day retention window, never a pending one.

The escape hatches keep chains alive instead of ending them: `brain habits skip`
(cadence-aware) and `defer_habit.py` push an occurrence forward, and
`brain habits revive` repairs a chain whose rows are all `done`. Retiring a habit
stays possible, but only as an explicit user decision through `/todo remove`,
never inferred from a row's age.

The recurrence rule this protects is deliberately cadence-preserving: the next
occurrence is the first rung of the `due_date + N × interval` ladder **strictly
after** today. A late completion therefore keeps its original cadence even when
that puts the next occurrence as soon as tomorrow (a 3-day habit due the 2nd and
completed the 4th comes due the 5th), and a rung landing exactly on today is
skipped for a full further interval. Anchoring to the completion date instead
would have drifted every weekly habit off its weekday, so cadence wins;
`recurrence_anchor` in `src/tasks/complete/tests.rs` pins each case.

## C7 — `tasks set` is absolute-value and defer-count-free; `--linear-issue` is a filter

Two capabilities existed only as bundled Python (`list_linked_tasks.py`,
`defer_task.py`), so anything outside the `/todo` skill had to shell into those
scripts or read `tasks.csv` directly. Both are now native, which is what lets an
external caller (a Linear-side skill, a cron job, another agent) drive Brain
through the CLI alone.

**Finding the local mirror of an issue is a filter, not a subcommand.**
`--linear-issue` joins the existing global `Filters`, so it composes with every
view and with `--include-done` / `--include-deferred` instead of inventing a
parallel lookup path with its own output shape. Reaching a closed or parked
mirror — the common case when reconciling a completed issue — is then just the
flags the caller already knows. Matching is case-insensitive exact on the column;
substring matching would let `AVA-17` silently claim `AVA-177`.

**Editing is absolute-value, and deliberately not a defer.** `defer_task.py`
exists to record slippage: it increments `defer_count`, which feeds the
high-defer warnings and the chronic-ignore sweep. But when a tracker moves an
issue's due date, or a title or priority changes upstream, that is not the user
avoiding the work, and counting it corrupts the signal those passes depend on.
So `set` writes exactly the columns named and never touches `defer_count`;
relative, penalty-counting pushes stay with the defer path. The two are different
operations that happened to share a column.

`set` reuses `add`'s flag names so one mental model covers create and edit, and
resolves rows through the same `locate` as `complete` (ID, bare number, or unique
fuzzy name). A no-op edit reports "unchanged" and writes nothing, so a mirror
pass can run repeatedly without churning the file or its `last_touched`. Per
C6, a habit row requires the explicit `--habit` opt-in: rescheduling a habit is
legitimate but must never be something a cleanup pass reaches by accident.

The pure/impure split is `set::plan` (every validation and the exact
before/after list) versus the thin read-modify-write in `set::set_in_root_with_today`,
so all the rejections are unit-testable without a filesystem.

## Env vs. config is a two-question test, and the daily-triage opt-out failed it

Two stores can hold a setting, and "it feels machine-ish" is not a criterion.
[docs/config.md](config.md#deciding-which-store-owns-a-new-variable) now states
the test explicitly, in order:

1. Does the value have to exist *before* a workspace does? Then it cannot live
   inside the workspace it bootstraps — it is **env** (`root`, `sync.*`).
2. Otherwise: must every machine connected to the workspace agree on it? If
   machines may differ, **env**; if they must agree, **config**.

The unit in question 2 is a machine, not a user: one person's laptop and desktop
are two machines on the same workspace, and so are two people's. That phrasing
matters because it explains results that "personal vs. shared" gets wrong. The
receiver credentials (`twilio_*`, `resend_*`, `brain_receiver_public_url`) look
shared — one workspace, one phone number — but exactly one machine serves
ingress for a workspace at a time, and the credentials belong to whichever
machine that is. They stay in env. `default_agent_frontend` is the same shape: a
machine that has only Claude installed must not be dragged onto Codex because
another machine prefers it.

The opt-out for the daily-triage nudge was a CLI flag (`--no-daily-triage-check`)
and failed the test in the other direction. Which shells may nag you about
today's triage is a property of the *workspace*, not of one invocation: if a user
does not want that modal, they do not want it on their laptop this morning and
their desktop tonight. It is now `enable_daily_triage_check` in portable config.

The palette toggle remains, because a TUI that stays open for days needs to
silence the nudge *now* without quitting. It **writes the config value** as well
as the live field: flipping it is the same decision as
`brain config set enable_daily_triage_check=false`, and a user who silences a
recurring nudge does not expect it back at the next launch or on their other
machine. (An earlier revision of this entry made the palette flip
process-scoped; that was wrong for exactly that reason.) A failed write does not
fail the toggle — the running session still honors it — but it is reported,
because quietly turning a persistent choice into a session-only one is worse than
either outcome. `App::skip_daily_triage_check` stays the single field both paths
reach, so they cannot drift.

Nothing is both a flag and a stored variable. A setting with two persistence
stories has two answers when they disagree, which is the exact failure the
CLI ↔ palette parity rule in `AGENTS.md` exists to prevent.

## Personas are keyed by person, and only your own is ever prompted for

`personalization.json` held one unowned object: a name, a role, an org, a
namespace list, tag styles. That schema quietly assumed a workspace has one
user, which stopped being true when workspaces gained portable members. Two
people sharing a family brain had one role and one org between them, and
whichever machine wrote last won.

So the store is now keyed by portable user ID — the same IDs as `users.json` —
with one `Persona` per member. Note that this is not the env-vs-config question
(see the entry above): a persona is not a setting a machine or a workspace holds
one of, it is a fact about a *person*, so the right key is the person. It lives
in portable config because everyone on the workspace should see the same people,
and identity, not location, decides whose values apply.

**Migration keys the legacy object onto its reader.** A version-1 file names no
owner, and the only person who can truthfully claim it is the local user of the
machine reading it: whoever wrote it was sitting at a machine, and on a
single-user brain that is the same person. Migration happens on read (so no
command has to run first) and persists on the next write. An *empty* legacy file
migrates to no personas at all rather than to one blank record — otherwise every
migrated workspace would immediately start nagging its user to fill in a persona
that was never there.

**Reads pick one person; the roster view shows everyone.** Tag styling and the
namespace checklist use the local person's persona, because those are how the
human at this terminal wants their own board and their own project buckets to
read. `brain persona list` is the all-members view, and it is what the bundled
skills call: an agent assisting in a shared workspace needs to know both who it
is serving (marked `(this machine)`) and who else's name might appear on a task.
The roster block deliberately includes members with no persona (as `(unset)`)
and personas whose user has left `users.json`, because a skill silently not
seeing a person is worse than seeing an empty one.

**Only the local person is prompted.** A missing persona is collected by
whatever `brain` command runs next, which is the only moment brain reliably has
the person's attention. Prompting for *another* member's persona on your machine
would be asking you to invent facts about someone else, so those surface instead
as the `other members' personas` optional feature in workspace status. The prompt never
fails the command that triggered it: with no terminal it degrades to one line
naming the fix. `brain persona …` and `brain workspace migrate` skip the gate —
one is already collecting the answer, and the other must not interleave prompts
with a transactional schema change.

The command is `brain persona`, with `personalize` kept as a hidden alias:
"personalize" describes an action on a singular *you*, which is exactly the
assumption that broke.

## `markdown_to_pdf_path` is machine-global, because a machine has one of them

Brain env was per workspace: every value lived in the selected workspace's
record, siloed so one workspace could never read another's. That is right for
almost everything it holds — a receiver URL, a sync block, provider credentials,
a frontend launch command are all things two workspaces on one laptop may
legitimately answer differently.

`markdown_to_pdf_path` was never one of those. It names where the
`markdown-to-pdf` binary is installed on **this machine**. Two workspaces on the
same machine pointing at different copies is not a configuration, it is a
mistake waiting to be discovered. The per-workspace shape produced exactly that:
auto-discovery ran per workspace, so the answer was stored once per workspace,
`brain env` listed the same path under every block as if each owned its own, and
a user who fixed the path in one workspace still had a stale one in the next.

`brain_receiver_public_url` joined it in v4. It was per-workspace only because
each workspace's webhook URL used to carry its own ingress path; once one
machine-wide URL per channel replaced that, a second answer for the origin would
mean a second URL, which is exactly what the change removed.

So schema v3 adds a top-level `env` map for values scoped to the machine, and
the env layer routes reads and writes by scope
(`env::schema::MACHINE_GLOBAL_VARS`); v4 hoists one more key through the same
rewrite. Because `-w` is accepted on every env command, a machine-global write
also *says* it landed once for the whole machine, rather than letting the
selector imply a scope it does not have. The question that decides scope is in
[config.md](config.md#deciding-which-store-owns-a-new-variable) as 2a: *could
two workspaces on the same machine sensibly hold different values?*

**The upgrade runs on the next ordinary command, not on request.** A schema bump
the user has to know about is a schema bump most users never perform. A v2 file
fails the exact-version check, which already routes through `env::migrate`; the
upgrade preserves every record untouched and moves only the hoisted keys. When
several workspaces carried a path, the first in canonical-name order wins. Any
of them would do — they name one binary — but picking *deterministically* means
the result does not depend on which command happened to trigger it, and a retry
after an interrupted run reaches the same answer. A blank value never displaces
a real one, since an empty string resolves as "unset" and hoisting it would lose
the only real path on the machine.

**Read-only probes upgrade in memory and write nothing.** `brain workspace list`
and the status commands are documented as literal read-only probes. Refusing to
report on an old schema would be the worst of both: the user updated Brain and
the first thing they typed failed, with no way forward that a status command is
allowed to take. So `RegistryStore::load_readable` applies the same pure upgrade
in memory, reports normally, and leaves the file for the next ordinary command
to rewrite properly — with its backup and its transaction.

## `brain workspace list` reports health for every workspace unless you name one

The list rendered every registered workspace's identity rows but only the
*selected* workspace's requirements, so on a two-workspace machine the second
one's feature health was invisible — including the setup it was still missing.
The header said "Workspaces" while the body answered a narrower question.

The selector decides the scope, which is what `-w` means everywhere else in
Brain: `brain workspace list` is a machine inventory and reports every
workspace's health; `brain workspace list -w family` asks about one and reports
one. A peer workspace gets its own read-only `CommandContext` built from its
record for the inspection; a peer that cannot be inspected (still needs setup,
unreadable root) renders a one-line note naming the repair command instead of
failing the command. The inventory is exactly where a user looks to discover
that a workspace needs setup, so a half-configured workspace must not be able to
take the listing down with it.

## A registered workspace is set up on first use, not reported as missing

`env.json` rides between a user's machines, so registering a workspace on one
registers it on all of them. The second machine then knew about `family` and
refused to do anything with it: "workspace root ~/family is unavailable; restore
it or detach the workspace". Every fact needed to fix that was already in hand —
the root path, the workspace UUID, and the sync credentials — so the message was
asking the user to perform a setup Brain could perform itself.

`initialize_workspace_directory` now runs before readiness on every ordinary
command: create the root, write the manifest from the UUID the record already
carries, sync, and seed PARA when there is nothing to pull. It is idempotent, so
the steady state costs two filesystem checks.

**The one thing it will not do is invent a root over a missing parent.** An
unmounted volume and a never-created workspace are indistinguishable from the
path alone, and creating an empty workspace over a detached drive would look
exactly like losing the data on it. A missing parent keeps the old error.

**The first sync from a machine is bidirectional.** Startup sync had always been
a pull, which is right for joining an established workspace and wrong for the
machine that *is* the workspace: content created before sync was configured had
never been uploaded, and no pull would ever upload it. The `family` workspace on
the author's laptop sat with an empty sync journal and an empty bucket for
exactly this reason. So the direction is chosen from whether this machine's
journal has any completed run: none means establish (both ways), otherwise pull
only when the root is empty, otherwise leave it to the ordinary startup pull and
the change-triggered push.

**Sync and registry commands opt out.** `brain sync` doing a different sync
first would sync twice and seed CSVs for its own pull to reconcile;
`brain workspace default` writing portable config as a side effect of a registry
edit surprised an acceptance test that (correctly) asserted the opposite.

## An empty read is not a corrupt manifest

`rclone cat` of a missing object exits 0 with no output on some backends. The
remote-identity probe took `success` as "a manifest was read" and parsed the
empty bytes, so a pristine bucket reported
`remote workspace manifest is invalid or incompatible: EOF while parsing a value
at line 1 column 0` and refused every sync — including the first one, which was
the one that would have created the manifest. A brand-new bucket could never be
initialized, and the failure named a data-integrity problem that did not exist.

A successful read of zero (or whitespace-only) bytes now falls through to the
listing, which classifies the remote as empty or manifestless exactly as before.
Bytes that are *present but malformed* still fail closed: those could be a
damaged ownership claim, and refusing is the safe reading. No bytes claim
nothing.

**The setup ownership claim needed the same reading, and did not get it.** The
claim election (`src/sync/identity/claim.rs`) reads
`.config/workspace-claims/<uuid>.json` before publishing, and took `success`
alone as "a claim is already there". On B2 that read succeeds with no bytes, so
the empty stdout was compared against the local manifest, disagreed, and setup
died on `remote workspace ownership claim does not match the local manifest` —
against a bucket holding nothing to disagree with. Every workspace initialized
after the election shipped in 0.35.1 was unable to run `brain sync setup` at all;
older remotes were unaffected only because they already carry a canonical
manifest and never reach the election. Both readers now share one rule, so a
blank claim read means *absent* and the publication proceeds. The post-publish
readback distinguishes the two failures too: a blank readback is a verification
failure that reports rclone's own stderr, not a phantom ownership conflict.

The generalization worth keeping: **whenever Brain decides something from a
remote read, the exit status is not the answer — the bytes are.** A fake that
models a missing object as an error hides exactly this class of bug, which is
why the identity test doubles now return exit 0 with empty output for a missing
object, matching B2.

## Codex could always resume; Brain just never asked

`CodexFrontend::command_for` had built `codex resume <id>` from the start, and the
session store recorded Codex ids like any other frontend. But
`resume_candidate_exists` and `can_resume_response_session` both returned a flat
`Ok(false)`, so no Codex session was ever *selected* for resume. The visible
symptom was an SMS or email conversation that could not continue once the panel
closed: each message began a new session with no memory of the last.

The missing piece was evidence. Claude resumes when a transcript file exists;
OpenCode asks its own API for live root sessions; Codex offers no
machine-readable session listing at all (`codex --help` has `resume` but nothing
like `session list`), so there was nothing obvious to validate against and the
safe answer was "never".

Codex does record every session on disk, as
`~/.codex/sessions/<YYYY>/<MM>/<DD>/rollout-<timestamp>-<uuid>.jsonl`, which makes
the check the same shape as Claude's: the session is resumable when its rollout
is there. Two details had to be settled empirically before trusting it, because
guessing either would have shipped a check that silently never matches:

- **Which id the filename carries.** A rollout's `session_meta` payload has both
  `id` and `session_id`, and they are *not* always equal — on subagent threads
  `session_id` is the parent. Across 400 real rollouts the filename UUID equalled
  `id` every time, and for top-level sessions `id` equalled `session_id` too.
  Since the session-start bridge ignores payloads carrying a parent, every session
  Brain registers is top-level, so matching the filename resolves the stored id
  whichever field Codex reported it under.
- **That matching must be exact.** The id has to occupy the whole trailing
  segment after a `-`. A prefix match would let one id claim another's rollout and
  resume a stranger's conversation, which is worse than not resuming at all.

The walk descends only to the day level and visits day directories newest-first,
so a live session is found in the first directory examined and an unexpected deep
directory cannot turn one resume check into a full-disk scan.

**Both channels read the same evidence.** `can_resume_response_session` delegates
to the same predicate as the interactive path, so SMS, email, and the panel cannot
drift apart about whether a session can be picked back up.

## An agent frontend's dependency tree is not workspace content

Running the brain panel on OpenCode made it install `@opencode-ai/plugin`'s
dependencies into the workspace it was running in: 3,649 files and 61 MiB under
`.opencode/node_modules`. Correct behavior for OpenCode, and nothing in the
bisync exclude set covered it, so the change-watcher saw thousands of new files
and pushed them. `family-brain` went to 3,286 objects of which **3,262 were
`node_modules`** (24 objects of actual content); `pablo-brain` reached 6,782
objects with **3,435 `node_modules`**, over half its object count.

That last figure recontextualizes the sync-performance work in the PM log: the
"6,700-object remote" whose listing cost motivated `--fast-list` was more than
half agent dependencies.

OpenCode states the intent itself — the `.opencode/.gitignore` it writes lists
`node_modules`, `package.json`, `package-lock.json`, `bun.lock`, and
`.gitignore` — but rclone does not read `.gitignore`. Those paths are now
excluded, with `node_modules/**` left unanchored so it matches at any depth,
whatever tooling a workspace grows. Brain's own `.opencode/plugins/brain.js`
bridge stays synced: that is content every machine needs, and a test pins the
distinction.

**The watcher had to learn the same set.** It mirrors the exclude list by
design, and mirroring it *partially* is its own bug: excluded writes that still
trigger produce a debounced sync that transfers nothing, once per agent launch.

The general rule this shares with the hook-script entry above: **anything Brain
or its tooling generates inside a workspace root is a candidate for exclusion,
because a workspace root is a sync surface, not a scratch directory.**

A deeper fix exists and was deliberately not taken: `LifecycleTarget::Home`
already exists, so the bridge could be installed into OpenCode's global plugin
directory instead, keeping the dependency tree out of every workspace entirely.
That changes a registered frontend's integration contract and depends on
OpenCode's global-plugin behavior, so it belongs in its own change with its own
verification rather than riding along with an exclude fix.

## The task store must exist before the first sync, not after it

Root initialization seeded the PARA skeleton and both task CSVs *after* the
startup sync, on the reasoning that a sync which pulls content makes seeding
unnecessary. That reasoning does not hold for the task store: `tasks/tasks.csv`,
`tasks/habits.csv`, and `tasks/SCHEMA.json` are all **excluded from bisync**, so
no sync can ever bring them down. The CSV lane creates the CSVs, and it reads the
schema document to decide how to merge.

So a machine joining an established workspace ran its first sync with no local
document at all: local read `Legacy`, the remote read `Current`, and the merge
refused. `tasks/` was then still completely empty, which made the suggested
`brain workspace migrate` fail too, on `reading required task schema input
.../tasks/tasks.csv`. A fresh machine had no way in.

The task store is now seeded before the sync, and the rest of the skeleton after
it. That split needs one care: emptiness is decided **once**, before Brain writes
anything, and that captured answer drives the later seeding. Re-checking
afterwards would see Brain's own `tasks/` files and skip the PARA directories,
lookup CSVs, and portable config.

**The remote's document wins when there is one.** `resolve_task_schema_document`
fetches it and only falls back to Brain's canonical copy when the remote has
none. A workspace may carry a customized schema, and the document is excluded
from bisync, so seeding the canonical copy over a customized remote would fork
the two with nothing able to reconcile them — the same trap as minting a
`receiver_ingress_id`. Verified against a live remote: a simulated joining
machine ended up with a byte-identical document and the same
`receiver_ingress_id` as its peer.

## Existence is not legacy-ness, and a dead end is never an acceptable state

Seeding the canonical `tasks/SCHEMA.json` flipped every local workspace to
schema `Current`. A remote that had never received the document still read
`Legacy`, because `remote_schema_status(None)` treats *no document* the same as
*pre-v2 document*. The CSV preflight compared the two and refused. The document
is excluded from bisync and was published only by `brain sync setup`, and
setup's own guard then refused too, because it keyed on `state.has_csvs` —
whether CSV files **exist** — as a proxy for whether the remote holds legacy
rows. The remote's CSVs were header-only with the current header: zero rows,
nothing legacy about them. So `brain sync` refused, `brain sync setup` refused,
and the message claimed the data was legacy when it was not. **No command the
user could run would fix it.**

Two rules came out of this:

- **Classify by content, not by existence.** `classify_remote_csvs`
  (`src/sync/csv_merge/remote_csvs.rs`) returns `Absent`, `Current`, or `Legacy`
  from what the CSVs contain. Empty or whitespace-only content proves nothing
  and must not veto initialization. Both the sync preflight and setup's guard now
  refuse only on `Legacy`. Setup pays two extra rclone runs to download the CSVs,
  and only when files are present, so the per-sync fast path is untouched.
- **Brain heals the remote, not just the local side.** When this machine declares
  the current schema, the remote has no document at all, and the remote's CSVs
  hold no legacy rows, the sync **publishes the document** — exactly what setup
  would have done. Requiring a separate command to repair a state Brain itself
  created is the failure, not the missing command. Genuine legacy rows and a real
  pre-v2 document still refuse, and now name `brain workspace migrate` as the
  remedy instead of asserting something false about the data.

The general principle: a state Brain can reach must have a path out that Brain
takes on its own. Auto-heal on sync and on TUI launch; ask the user for nothing.

## A joining machine must adopt portable identity, never mint it

A second machine with the same registry could not open a synced workspace at all.
It created the root, ran the startup sync, and died on `validate local workspace
manifest`, because the sync's identity gate reads `<root>/.config/workspace.json`
and root initialization wrote that file *after* the sync. The step that creates
the identity ran after the step that refuses to run without it.

Reordering alone would have been wrong, and quietly so. `WorkspaceManifest::new`
issues a **fresh `receiver_ingress_id`**, and `.config/workspace.json` is the
first entry in the bisync exclude list — "the separately identity-gated portable
workspace manifest". So a locally minted manifest carries a different receiver
ingress identity from its peers for the same workspace, and the one file that
could correct it is the one file bisync never touches. It would have looked like
success and forked portable identity permanently.

Every other identity write publishes local → remote; nothing ever brought the
manifest *down*. `src/sync/identity/adopt.rs` adds that missing direction:
when the root has no manifest and sync is configured, Brain reads the remote's
manifest, refuses it unless its `workspace_id` matches the registry's, and writes
it locally, preserving `receiver_ingress_id`. Minting from the registry UUID
survives only as the fallback for a remote that has no manifest either, which is
a genuinely new workspace. Identity is now resolved *before* the first sync.

`tests/root_creation.rs` had covered a machine joining a workspace registered
elsewhere, but its own comment scoped it: "With no sync configured, setup falls
back to seeding PARA and the CSVs." The unsynced case, the one that never needed
to adopt anything, was the only one tested.

**Reading a remote file has one rule, in one place.** This was the third lane to
re-derive "success plus no bytes means absent" (`src/sync/identity/read.rs` now
owns it, and the claim lane delegates). The manifest probe learned it, the
ownership claim learned it separately and got it wrong, and adoption would have
been next.

**The failure also has to say what failed.** `validate_local_manifest` wrapped a
perfectly descriptive `ManifestError` in a `context` string, and the workspace
renderer prints only the outermost `Display`, so the user saw four words. Folding
the cause into the message — rather than switching the renderer to `{:#}` — was
the right fix: the renderer's curated top-level messages are deliberate, and a
blanket `{:#}` appended internal cause text to authored messages like the
manual-cleanup warning, which a test correctly caught.

## Nothing created the file every schema decision requires

`initialize_if_empty` seeded a new workspace's PARA tree, both task CSVs, both
id counters, and two lookup CSVs. It never seeded `tasks/SCHEMA.json`, and no
template for one existed anywhere in the repo. Every schema decision reads that
document through `read_required`, so `brain sync setup` died in its baseline
stage on a bare `fs::read` error naming a path, with no hint that the file was
supposed to exist or what should be in it. **Any workspace Brain created itself
could not complete sync setup.** The original workspace was unaffected only
because it predates the workspace system and carries a hand-written document.

Two details made this worth writing down:

- **The workspace was already schema-current; it just could not say so.** The
  seeded CSV headers carry `task_uuid` first plus `task_id`, `assigned_to`, and
  `system_key`, which is exactly what `csv_has_current_identity` wants. Only the
  declaration was missing, so the fix is a document, not a data migration.
- **The one existing document had silently drifted.** It declared `assignee`
  long after the CSVs moved to `assigned_to` — a column
  `csv_has_current_identity` explicitly *rejects* — omitted `task_uuid` and
  `system_key`, and documented a `backlogged_date` that no header has. Its
  version metadata was migrated; its column documentation was not. So the
  canonical document is generated from the same headers Brain writes, and a test
  asserts the documented columns equal those headers and are all known columns.
  Schema documentation that drifts from the columns Brain actually writes is
  worse than none, because it is read as authoritative.

Brain now embeds the canonical document and seeds it write-only-when-absent, the
rule the portable manifest already follows, on both the empty-workspace path and
the ordinary root-initialization path so an existing workspace repairs itself.
**Sync needed its own seed call.** Sync dispatches before the workspace gate, so
it never passes through root initialization: seeding only there fixed every
command except the one that was broken. `command::sync::run` seeds too, which
covers `setup`, `repair`, `status`, and a plain run from one place. A self-heal
that misses the command that needs healing is not one.
`RequirementScope::TaskSchema` reports a workspace still missing it, because
`sync status` had been printing `✓ cloud sync: ready` for a workspace whose next
sync could not possibly succeed. A health check that can say `ready` about a
guaranteed failure is not a health check.

The bundled document stays generic per the house rule: no personal categories,
no names, no absolute paths, no external tracker keys. A guard test enforces it.

## A suggestion without the selector is a suggestion to the wrong workspace

`workspace::suggest` exists so a message composed while `family` is selected says
`brain sync setup -w family`. The rollout was partial, and the gap showed up in
the worst place: the staged-claim message a first-time `brain sync setup -w family`
prints told the user to "run `brain sync setup` again", which on their machine
targets the default workspace. The refusal one step earlier had it right, so a
single screen contained both spellings.

Nineteen literals across sync, tasks, persona, and config bypassed the helper.
They now route through it, and `tests/workspace_suggestion_selector.rs` scans
production source so the next one fails the build instead of a user's afternoon.
The guard needs three judgment calls encoded rather than guessed:

- **Which families are workspace-scoped.** `sync`, `config`, `persona`, `tasks`,
  `habits`, `todo`, `reindex`, and `user` resolve one workspace. `env`, `skills`,
  `server`, and `workspace` are machine-local or registry-level, where a selector
  would be noise, so they are excluded rather than allowlisted case by case.
- **Naming a command is not suggesting one.** "`brain sync init` was renamed to
  `brain sync repair`" must stay bare, so those two literals are listed
  explicitly with that reason attached.
- **Where the source ends.** Cutting each file at the first `#[cfg(test)]`
  looked right and silently skipped everything after an early
  `#[cfg(test)]` helper — including the offending line in
  `src/sync/identity/mod.rs`, so the guard's own first run passed over the bug
  that prompted it. It now cuts only at the trailing `#[cfg(test)] mod tests`.
  A source-scanning guard that quietly reads less than it claims is worse than
  no guard, because its green is load-bearing.

## Brain was pushing its own hook scripts to the cloud on every launch

Every TUI startup reinstalls the lifecycle artifacts (`.brain/hooks/*.py`,
`.claude/settings.json`, `.codex/hooks.json`, `.opencode/plugins/brain.js`) by writing a temp file and
renaming it over the destination. That is correct for atomicity and wrong for
idempotence: renaming gives the file a fresh mtime even when the bytes are
identical. Those files live inside the workspace root, so the change-triggered
watcher saw a change, debounced, and pushed — every launch, forever.

The B2 version history showed it plainly: three launches in one afternoon, three
uploads of byte-identical hook scripts. It also explained a confusing symptom —
"why does the TUI push before it pulls?" The startup pull and the watcher's push
are two independent one-way syncs racing for the workspace sync lock, and the
push kept winning because it was triggered by writes Brain itself had just made
while the pull was still listing a 6,700-object remote.

Installation now compares first and writes only on a difference, so
reinstallation is idempotent on disk as well as in content. The general rule for
anything Brain writes inside a workspace root: **a rewrite is a change to
everything watching the tree, so write only when the content actually changed.**

## Sync output names findings and decisions, not just steps

"Starting rclone sync; live file progress follows…" followed by a long silence is
indistinguishable from a hang, and identical whether the run moved 400 files or
none. Each phase now reports what it *found* and, where it branches, what it
*decided*: the identity probe says the remote belongs to this workspace, the file
sync says how many files differed (or that none did), the merge says how many
rows differed, and a skipped phase says why it was skipped. The counts come from
the same `RunOutcome` the journal records, so the narration cannot drift from
what was persisted.

This is also what makes a live log worth reading, which is what the palette's
**Show sync status** modal tails.

## The sync-log modal shows only a running sync

`Show sync status` used to set a one-line flash ("syncing now (pull)"), which
answered whether a sync was running but not what it was doing. It is now a modal
over the running sync's `current.log`, re-read every frame so it tails rather
than snapshots.

It deliberately refuses to show an *older* run's transcript. A finished run's log
answers "what happened last time?" while looking exactly like the answer to the
question the user asked by opening it — "what is happening now?" — and a stale
transcript that looks live is worse than no transcript. With nothing running the
modal says so in one line. The status line stays a one-liner; the modal is the
place for detail.

## Brain's own children must name their workspace

A two-workspace machine makes a missing `-w` dangerous rather than merely
imprecise: a command that means `family` and omits the selector silently syncs,
reindexes, or mutates `brain`. Reviewing call sites catches that once; it does
not keep catching it.

So Brain sets `BRAIN_REQUIRE_WORKSPACE` on every child it spawns for its own
work, and a process that sees it refuses to run without an explicit
`-w`/`--workspace`. Any code path that builds a `brain …` command and forgets the
selector now fails loudly the first time it runs, in development, with a message
naming the cause.

It is deliberately scoped to Brain-spawned children. A person typing `brain sync`
should get the default workspace — that is what a default is for — and forcing
`-w` on agent- and skill-issued commands would break every bundled skill. The
agent panel already has a stronger guarantee anyway: it receives
`BRAIN_WORKSPACE_ID`, so a command that resolves a different workspace fails on
identity rather than on a missing flag.

## `BRAIN_WORKSPACE` selects, not just describes

Every process Brain launches already received `BRAIN_WORKSPACE` (the canonical
name) and `BRAIN_WORKSPACE_ID` (the UUID). Only the UUID was *used*, and only to
validate: bootstrap selected a workspace from `-w` or the machine default, then
refused if the result disagreed with the launching UUID.

That made the common case fail rather than work. A bundled skill that runs
`brain config get …` inside a `family` panel passes no selector — 97 of the 98
`brain` invocations across the bundled skills don't — so bootstrap selected the
default workspace and then aborted on the identity mismatch. Safe, in that it
never operated on the wrong brain, and useless, in that the skill just broke.

`BRAIN_WORKSPACE` is now an implicit selector: explicit `-w` wins, otherwise a
Brain-launched process inherits the workspace it was launched for, otherwise the
machine default. It is resolved once in `bootstrap` by filling the parsed
`Cli::workspace_selector`, so selection, readiness, scope checks (`is_some()`),
and suggested commands all read one answer instead of each deciding again. The
UUID check keeps its job: it validates the resolution rather than substituting
for it.

An environment variable is the right carrier precisely because of subagents. They
run in their own shells, and a shell inherits its parent's environment, so any
descendant of an agent panel gets the same workspace without cooperating. The
alternative — teaching 98 skill invocations to pass `-w "$BRAIN_WORKSPACE"` —
would have to be re-taught to every skill anyone ever writes, including the ones
in a user's own `~/brain/.config/plugins/`.

The remaining case — a skill running in a shell Brain never launched — is covered
by current-directory discovery, in the entry below.

## The current directory selects a workspace, the way git finds a repository

`BRAIN_WORKSPACE` covers every process Brain launches. It does not cover the case
where someone starts an agent themselves in `~/family`, or simply types a command
there: with no variable set, `brain sync` acted on the machine default, which on a
two-workspace machine is the wrong brain.

So with neither a flag nor an inherited workspace, Brain walks up from the current
directory and selects the workspace whose registered root contains it. This is
git's rule, and it is chosen for the same reason: the directory you are standing
in is the most reliable available statement of what you are working on, and it
needs no flag, no variable, and no cooperation from the tool being run.

**Nearest ancestor, not first match.** The registry already forbids overlapping
roots, so at most one can be an ancestor today — but the rule is written as
nearest-ancestor anyway, because a rule that only happens to be unambiguous is a
rule that breaks quietly when the constraint changes.

**Symlinks are resolved before comparing.** On macOS a root registered as `/tmp/x`
and a working directory reported as `/private/tmp/x` are the same directory, and a
lexical comparison silently fails to discover it. Both sides are canonicalized,
falling back to the literal path when canonicalization fails (a root on an
unmounted volume, say), which discovers nothing rather than guessing.

**The launching workspace outranks the current directory.** An agent panel opened
for `family` that reads a file under `~/brain` must stay on `family`: otherwise a
`cd` mid-session silently retargets every later command, and the resolution would
contradict the `BRAIN_WORKSPACE_ID` the panel carries — turning a working command
into an identity failure. Precedence is therefore flag, then launching workspace,
then current directory, then machine default: each step is a more specific
statement of intent than the one below it.

**Strict mode is unaffected**, and is checked before any of this. Its job is to
catch a Brain-built command line that forgot `-w`, so it looks at what the command
line actually said — not at what discovery could have recovered. A forgetful code
path that happened to run inside the right directory would otherwise pass and stay
broken.

## rclone marches per directory, which is why sync was slow

A no-change `brain sync` took 19.4 s, of which an equivalent dry-run bisync was
17 s. The first guess — that per-process rclone authentication dominated — was
wrong, and an early measurement made it look like `--fast-list` did not help:
`rclone lsf --recursive` is the same speed with and without it, because that
command already lists recursively.

bisync does not. Its `march` walks both sides **directory by directory**, and on
a bucket backend each directory level is its own list API call. With ~1,000
directories in the workspace, that is ~1,000 round trips to enumerate 6,769
objects that a single recursive listing returns in 2 s.

`--fast-list` is now passed to both remote walks (`bisync_args` and the
change-triggered `push_args`):

| | Time |
| --- | --- |
| Dry-run bisync, default march | 15.6 s |
| Dry-run bisync, `--fast-list` | 6.9 s |
| No-change `brain sync`, before | 19.4 s |
| No-change `brain sync`, after | **7.2 s** |

Its documented cost is memory — the whole listing is held at once — which for
thousands of objects is nothing. Two other candidates were measured and rejected:
`--checkers 32` changed nothing, and excluding the large media library was
*slower*, which confirms the constraint was round trips rather than object count.

The lesson worth keeping: a listing benchmark is only evidence about the command
benchmarked. `lsf` and `bisync` enumerate differently, and testing the cheap one
answered a question nobody had asked.

## The daily-triage nudge shows immediately and is withdrawn if the sync disproves it

The nudge used to wait for the startup sync, so on a sync-configured workspace it
appeared some seconds into the session — after the point where you have started
reading or typing, which is exactly when an interrupting modal is worst. It waited
for a good reason: another machine may have completed today's triage, and the
local `habits.csv` will not know until the pull lands.

Both properties are now available by separating the question from the answer. The
nudge is raised at startup from local state, and the post-sync refresh
*reconciles* it: `resolve_triage_alert` compares whether triage is still
outstanding against whether a nudge is on screen, and opens, withdraws, or leaves
it. A withdrawn nudge flashes why, so the modal disappearing is explained rather
than mysterious.

Withdrawing is the important half. Leaving a stale nudge up invites the user to
answer a question that has already been answered — and answering "yes" re-runs a
triage pass that already ran on another machine. The dismissal only fires for a
`ConfirmKind::RunTriage` modal, so an unrelated confirmation the user opened in
the meantime is never closed under them.

## Why inbound email identities are parsed as mailboxes, not addresses

`normalize_email` deliberately refuses to guess: it rejects any value with
whitespace, so `pablo@example.com` normalizes and anything else fails. That is
right for a *configured* identity, where a person typed the value and an
ambiguous one should be a validation error. It is wrong for a value lifted out
of a mail header, because `From`, `To`, and `Cc` carry RFC 5322 mailboxes —
`Pablo Sarmiento <pablo@example.com>` — far more often than bare addresses.

Applying the configuration rule at the provider boundary produced two failures
that both look like "email is broken" and neither of which logs a cause. The
sender was rejected as not-allowed, so a normal email from a normal mail client
got HTTP 403 and never reached an agent. And every thread participant failed
the allowlist intersection, so on the paths that did run, the reply had no
recipients and was discarded.

`normalize_mailbox` reduces a mailbox to its addr-spec and then delegates to
`normalize_email`, so exactly one rule decides what a valid address is. The
boundary applies it (`fetch_verified` for the sender and participants,
`allowed_thread_recipients` for both sides of the intersection and the
receiving address); configuration still uses `normalize_email` and still
refuses to guess. This is the email analogue of the E.164 work on the SMS side:
accept the shape the provider really sends, normalize once, compare normalized.

The receiving address matters for the same reason. It is the self-echo guard,
and it comes from free-form env, so a perfectly reasonable
`resend_from_email` of `Brain <brain@example.com>` would have compared unequal
to the address it names and let brain answer its own mail.

## Why an inbound email is converted to text and bounded

The prompt is typed into the brain panel's PTY. Two properties of email make
that unsafe without shaping. Mail from a rich client is often HTML-only, and
the raw markup buries the actual message; and the receiving API's cap is 1 MiB,
which a newsletter reaches easily, so an unbounded prompt is an unbounded PTY
write. `body.rs` therefore drops `script`/`style` bodies, keeps element text
with block boundaries as line breaks, and caps the result at 16 KiB with an
explicit truncation notice — the agent is told the message was cut rather than
answering a silently shortened one as if it were complete. A plain-text part,
when present, is still preferred and passed through verbatim.

## Why every outbound email reply goes through one seam

Three sites delivered email (the processing notice, the final response, and the
post-teardown fallback), and all three guarded on a non-empty recipient list
with no `else`. An empty list is the worst outcome this channel has: the user
gets nothing, which is indistinguishable from the agent never finishing, and
nothing in the log says why. `App::send_email_reply` is now the only path, and
it logs the drop with the two configuration fixes that resolve it.

## The receiver's own address is not a secret it should hide from its owner

Requirement status deliberately reports provider values as present or absent
and never prints them, and that rule is right for a credential. It was also
being applied to `twilio_from_number` and `resend_from_email`, which are not
credentials: they are the number and the address the receiver *publishes*, the
ones a person has to type into their phone or their mail client to reach it.
Answering "is it set" and refusing to answer "what is it" left the owner of the
workspace re-reading `brain env get` for a value brain already knew.

`brain receiver email` and `brain receiver phone` print exactly that value,
bare and unstyled on stdout, because the point of naming a channel is to pipe
the answer into something else. Routing by destination made the value load-
bearing as well as publishable — it is what selects this workspace out of every
workspace sharing the machine's one URL — which is one more reason the owner
gets to read it rather than only confirm it exists. A missing address is an error that names the
variable and both ways to set it, mirroring `receiver url`'s missing-origin
message rather than printing an empty line. The three real secrets
(`twilio_auth_token`, `resend_sending_api_key`, `resend_full_access_api_key`,
`resend_webhook_signing_secret`) are
still never printed by any of these commands, and an integration test asserts
it.

## Bare `brain receiver` is machine-wide because receiver configuration is

Every other receiver subcommand acts on the selected workspace, which is
correct for a mutation: enabling ingress for the wrong workspace is a real
mistake. But one machine hosts several workspaces, each with its own providers
and published addresses, and the question the bare command answers — what is my
receiver set up as — is almost never about exactly one of them. So bare
`brain receiver` opens with the machine's own webhook URLs, which belong to no
workspace, then reports every registered workspace and lets `-w` narrow those
blocks, the
same shape `brain workspace list` already uses, down to sharing its
`workspace::peer_context` helper for building a read-only context per record.

That sharing carries the failure rule with it. A half-configured peer must not
take the inventory down: an unreadable record reports `unavailable` with its
repair command and the listing continues. The liveness probe follows the same
principle in the other direction. When the shared process cannot be asked, the
block says `live state unavailable` instead of printing `Server not running`,
because inventing a fact about a process nobody reached is worse than admitting
the gap — the user would go looking for a stopped server that is running fine.

## A receiver command must be the whole message, and the two act at different times

A sender with no screen has exactly one input: the message body. So brain reads
`/new` and `/restart` out of it — but only when the command is the *entire*
message. Matching a prefix or a substring would let "what does /new do?" and
"restart the sync and tell me how it went" be silently obeyed instead of
answered, and the sender would get no reply to a question they are waiting on.
On this channel, swallowing a real message is the worst available failure, so
the match is exact (whitespace and case forgiven, because a phone adds a
trailing newline and capitalizes the first letter without being asked).

The two commands are applied at different points, which looks inconsistent
until you ask what each is *for*.

`/restart` is a rescue. Its entire value is immediacy: the sender is stuck
behind a backlog and wants out. Queueing it behind the very backlog it clears
would make it a no-op, so the durable coordinator finds and applies it before
any new dispatch gate. It still does not interrupt the answer in
flight (that message is being worked on, not stuck), and it keeps anything
sent after it, because that is work nobody asked to abandon. The cut is through
the queue, not a wipe of it.

`/new` is a conversational boundary. Its entire value is *where* it lands, so it
waits its turn and is applied only between messages. Applied immediately it
would cut mid-answer, discarding a reply someone is already waiting for and
placing the boundary somewhere the sender did not choose.

The commands are represented as durable jobs, not runtime-only intentions.
`/new` atomically retires only the exact logical conversation key, creates an
empty unbound replacement, and moves later unclaimed work in that conversation
onto it. `/restart` atomically fails only older unclaimed queued or
pre-acceptance retry rows, then establishes the same fresh boundary. The active
owned run, later arrivals, and every unrelated actor, channel, conversation,
and workspace remain unchanged. Retired conversation rows, transcripts, and
bindings are not deleted, so a mistaken command does not destroy history.

The restart scan and ordinary claim are separate event-loop stages, so their
database ordering must close the ingress gap between them. The claim uses
`BEGIN IMMEDIATE` and checks queued exact restart controls before selecting a
FIFO candidate. SQLite therefore produces two valid orders: ingress commits
first and the claim refuses, or the claim commits first and later ingress
preserves that legitimately active job. There is no order in which a restart
is visible yet older backlog becomes newly active. A claimed `/new` is likewise
allowed to finish after disable, but its success path rechecks intent before
claiming anything from the fresh conversation.

## Email markdown is rendered by a parser we did not write

SMS and email are opposite problems. A phone renders nothing, so markdown is
stripped. An email client renders HTML, so markdown had to be *translated* —
and until now it wasn't: the HTML part escaped the answer and wrapped each
paragraph in a `<p>`, which means every reply arrived with its hashes,
asterisks, and bracketed links intact, as visible punctuation. A structured
answer — the case where email is chosen over SMS in the first place — read
worst of all.

The strip pass (`reply/plain_text/`) is deliberately *not* a parser: it is
line-oriented, cannot fail, and is allowed to miss things, because on SMS a
stray marker is a cosmetic loss. HTML has no such tolerance. Getting nesting,
tables, fences, and entity escaping right is a CommonMark implementation, and
writing one to send email is not this project's work, so `pulldown-cmark` does
it. That asymmetry is the decision: the medium that tolerates being wrong gets
our own code, the medium that does not gets a library.

Two things the library correctly leaves to the caller are handled in
`reply/html.rs`. `pulldown-cmark` passes raw HTML in the source straight
through, which is right for a document you wrote and wrong for one that quotes
an inbound SMS: those events become text, so markup is shown, not run. Link and
image destinations are checked the same way and dropped unless they are
`https:`, `http:`, or `mailto:` — a link is the one thing in an email a reader
is invited to click.

Element styling is a `<style>` block rather than inline `style` attributes.
Inlining survives more mail clients, but reaching it means emitting every tag
ourselves, which is the renderer we just declined to write. A `<style>` block
is honored by Gmail, Apple Mail, and Outlook.com, and where it is ignored the
reply degrades to a correct unstyled HTML document — still structured, still
clickable. Losing the styling is an acceptable worst case; losing the structure
is what we were fixing.

## SMS markdown is stripped in code, not merely asked for in the skill

The bundled skill now tells the assistant to write SMS as plain text, and that
guidance is worth having: text written for a phone is shorter and clearer than
markdown with its markers deleted. But a prompt is a request, not a guarantee.
Models trained to format helpfully will occasionally emit `## Today` and
`**Rent**` anyway, and on SMS the reader sees the punctuation literally. So the
guarantee lives in `server/reply/plain_text/`, a pure pass every outbound SMS
goes through, and the skill's instruction is the optimization on top of it.

Placing it inside `reply::sms` rather than at the three delivery call sites is
what makes it total: the final response, the fallback PTY-scrape response, and
any future SMS body all shape through one function, so no path can be added
that quietly posts raw markdown. Stripping precedes the length check for the
same reason it exists — four asterisks that render as nothing must not be what
pushes a 480-character answer into a truncated one.

The pass is deliberately not a markdown parser. It is line-oriented, cannot
fail, and never drops text it did not recognize: an unpaired `**`, `2 * 3`, and
`snake_case_name` survive verbatim, because mangling a user's arithmetic or an
identifier is a worse outcome than leaving one stray marker. Code-span and
fenced content is passed through untouched for the same reason — inside code the
markers are data. Where the medium genuinely cannot carry structure, the pass
converts rather than deletes: bullets flatten to one `- ` line (indentation a
phone cannot show is pure character cost) and table rows become comma-separated
cells.

Links are the one place the pass deliberately spends characters instead of
saving them. `[the invoice](https://…)` reduced to `the invoice` is shorter, but
a phone reader cannot click a label, so the answer becomes unactionable — the
opposite of the goal. The URL is therefore kept after the label, and only a
target that is genuinely reachable from a phone qualifies: a relative or local
path (`../areas/money/a.md`) is unreachable there, so it is dropped as noise.

Email is untouched. It has a renderer, `reply::email_html` already builds the
HTML part, and the plain-text alternative there wants the full answer, not a
de-marked one.

## A config variable that another store answers must say so

`brain receiver setup` writes portable `users.json` and never touches
`config.json`. `brain config` resolved `response_email`,
`allowed_sms_senders`, and `allowed_email_senders` from `config.json` alone, so
a receiver that had just been configured end to end — provider credentials,
webhook URLs, an inbound-allowed phone and email — reported all three as
`(unset)`. That output is indistinguishable from a workspace nobody ever set
up, and it is the output a user checks first when they want to confirm setup
worked. The setup had worked; the report was wrong.

Marking the rows "legacy migration input" in the description column did not
save it. A value column that says `(unset)` is read as the answer, and no
amount of explanatory prose next to it competes with a wrong value.

So `brain config` resolves these three from the live roster first and falls
back to `config.json` only when no portable user answers, which is precisely
the pre-migration case that key exists to serve. Three consequences follow.
`brain config get` returns the enforced value, so an agent querying one of
these is not told a stale legacy string. `config list` prints a muted note
naming `users.json` and `brain user`, because a value whose store you cannot
guess is only half an answer. And `brain config set` now **refuses** all three:
writing them to `config.json` persists something nothing enforces, which is the
same silent no-op in the opposite direction — the user would have every reason
to believe they had just set an allow list.

Only `inbound_allowed` identities are reported. That flag is the entire
authorization decision, so rendering a listed-but-disallowed number as an
allowed sender would misreport security state, which is worse than the
`(unset)` this replaced.

## A machine picking its person should be offered the roster, not quizzed

Readiness repair only asks for a local user when the workspace already has
members and none of them is this machine's — a two-person `family` root on a
second laptop, or any workspace whose `local_user_id` was cleared. It asked
`Local user ID (for example, pablo):` and accepted free text, so the one thing
brain knew and the human might not — who is actually in this roster — was the
one thing it did not say. A wrong answer was not rejected at the prompt either;
it travelled into the repair transaction and came back as `local user 2 is not
a portable member`.

So the prompt lists them: `Who is this machine?` with `<n>) <id> (<name>)` per
member, answered by row number. An exact ID still works for anyone who knows
it, and an answer matching neither re-asks rather than ending the command the
user actually ran — being unable to name yourself is not a reason to abandon a
`brain receiver setup`.

The numbering and answer-interpretation are the helpers `brain user` already
used, moved from `command::users::select` down into `users::select` so both
callers share one behavior. A prompt that numbers rows differently from the one
next to it is its own small betrayal.

The fallbacks stay narrow and unchanged. A sole member is still adopted with no
prompt at all, headless still errors with the exact repair commands, and a
roster that cannot be read falls back to the plain ID prompt — readiness repair
is the last thing that should fail.

## One receiver URL per machine, routed by the address the message arrived at

Every workspace's webhook URL used to carry its own opaque ingress:
`https://host/w/<ingress>/sms`. That made the URL self-selecting, which is why it
was built that way. It also made the URL the thing a user had to keep straight.
Setting up a second workspace meant fetching a second pair of URLs, pasting a
different one into each provider portal, and never mixing them up — while the
Twilio number and Resend address *already* differed per workspace and already
identified them unambiguously. The ingress in the path was a second identity for
a question the payload had already answered.

So there is now one URL per channel for the whole machine, `<public-url>/sms` and
`<public-url>/email`, and the workspace is selected from the destination the
provider names: Twilio's `To` against `twilio_from_number`, a Resend payload's
`to`/`cc` against `resend_from_email`. One portal entry per channel, for every
workspace, forever. `brain_receiver_public_url` became machine-global in the same
change, because "one URL" and "a per-workspace origin" cannot both be true.

**Routing reads the payload before the signature is verified, and that is safe.**
It has to: with no identity in the path, the destination is only knowable from
the body. What the unverified destination buys is exactly one thing — which
workspace's signing credential the request is then checked against. A forged
request naming another workspace's number still has to carry that workspace's
own valid signature, so it is rejected exactly as an ingress-scoped forgery was.
Nothing is read, loaded, or delivered on the strength of the unverified value.

The one gap that argument leaves is a machine whose workspaces share a provider
account, and therefore share a signing credential: there, a signature alone
cannot tell two workspaces apart. So after verification, brain re-checks the
now-authenticated destination against the routed workspace's own published
address (`confirm_destination`). That also closes the narrower race where the
registry changes between routing and credential load. Two workspaces publishing
one address is refused outright as ambiguous rather than delivered to whichever
sorted first: guessing there would hand one workspace another's private message.

**What this costs.** A provider request's body is now read before brain knows
whether any workspace answers, so an unavailable or unknown destination no longer
rejects before body IO — only local routes still do. The read stays bounded by
the same 1 MiB limit and the same connection deadline, and the control loop keeps
answering while a body is outstanding, which is the property that actually
mattered. The ingress itself is not gone: it still identifies local
capability URLs and remains the lease-table key a routed provider request
resolves through.

## Injected prompts are pasted, not typed

This remains the ordinary interactive `AgentController` input contract.
Receiver runs no longer inject an existing panel; they pass the initial prompt
to a new isolated launch.

A prompt Brain injects into an open panel used to be typed character by
character, with each newline encoded as `ESC CR` — the "insert a literal
newline, don't submit" chord all three frontends accept. That works only while
the composer is a plain text field.

With Claude Code (or Codex) in **vim mode** it fails completely, and it failed
silently. The `ESC` leaves insert mode, so everything after the first newline is
executed as normal-mode commands: motions, a stray `i` that re-enters insert
somewhere unintended, and a final `Enter` that never submits because the
composer is no longer where the sequence assumed. The visible symptom is a
receiver prompt sitting half-written in the composer forever. Nothing errors —
the bytes were delivered, the transport succeeded — so the turn simply never
starts, `brain_turn_active` stays pinned, and every message queued behind it is
answered with the processing notice and nothing else.

Every receiver prompt contains a newline (the actor preamble is separated from
the message body by a blank line), so this hit **every** message that reused a
warm panel. A message that launched a fresh panel was unaffected, because that
path passes the prompt as a command-line argument and never types anything.
That asymmetry is why the first SMS of a session answered and the rest did not.

So text is now delivered as one **bracketed paste**. It is the mechanism
terminals already use to hand an application clipboard content that must not be
read as keystrokes, all three frontends enable it (verified by probing each one
for `ESC[?2004h`), and it removes the `ESC` entirely rather than trying to
out-guess an editor mode. The submit key still lands as a real keystroke, after
the paste closes. Control characters are stripped from the payload so inbound
message text cannot close the paste early and have its remainder run as
keystrokes — the payload is attacker-influenced, since it is someone else's SMS.

The general rule this encodes: **injected content is data, and it must be
delivered through a channel that cannot reinterpret it as control.** Typing is
that channel's opposite.

## The submit key needs its own write, after the paste has landed

This remains the ordinary interactive follow-up contract. Receiver runs no
longer reuse a warm panel or submit a typed follow-up.

Pasting fixed the vim-mode corruption but not the whole bug. A prompt injected
into a warm panel still went unsubmitted, now with the composer holding the
text *correctly* and no turn behind it. Reproduced deterministically against a
real Claude Code PTY: send `ESC[200~…ESC[201~\r` as one write to a panel whose
previous turn just finished, and the text lands while the `\r` does nothing.
The same bytes submit fine on a panel that has never run a turn, which is why
the first message of a session always worked and a follow-up did not — the same
asymmetry as the vim-mode bug, from a different cause.

A terminal frontend handles the two on different paths. The paste is
accumulated and applied to the composer as a state update; the keystroke is
dispatched straight to the focused handler. When they arrive in one read, the
key can be handled against a composer the paste has not been applied to yet, so
`Enter` submits an empty composer (a no-op) and the text appears immediately
afterward, stranded.

So a follow-up is now **two writes**: the paste, then the key after a
`PASTE_SETTLE` pause. Measured on Claude: sharing the write loses the submit
every time, and a separate write 400 ms later always lands. The same probe run
against Codex and OpenCode did *not* reproduce the loss — both submitted either
way — so this is a Claude flaw as of the builds tested. Every frontend is paced
regardless: the cost is 400 ms on an injected follow-up, the alternative is a
per-frontend exception that has to be re-verified on every upgrade, and the
failure it prevents is a silently swallowed message. That made `InputSequence` a
list of `InputWrite`s rather than one byte buffer — pacing is part of what an
input *is*, not something a call site should improvise — and the wait belongs
to `PtyPane`'s existing writer thread, so the UI thread that queues a prompt
never blocks. Every frontend pays the same 400 ms, on injected follow-ups only:
nothing a human types is paced.

The general rule: **when a frontend's input paths can reorder relative to each
other, order in the byte stream is not order of effect.** Separate the writes
that depend on each other and let the earlier one take effect first.

## A dispatched turn that never answers must not strand the queue behind it

This warm-panel timeout policy is retained as historical rationale. Isolated
receiver runs now treat child exit without exact completion as a durable
pre-acceptance retry, while a live run is governed by its exact claim.

The bug above exposed a second, independent one. Nothing released an in-flight
receiver turn except a completion signal. The inactivity lease looks like a
timeout but is not one: `expired` only fires once `receiver_started` is `None`,
so it governs an idle warm panel, never a dispatched message. A turn that
crashed, wedged, or was never submitted therefore pinned the panel forever, and
every later message waited behind it indefinitely while its sender kept being
told the answer was still coming.

`remote_turn_timed_out` gives up on such a turn after ten minutes — comfortably
longer than the two-minute processing notice, so a genuinely slow answer is told
it is still coming long before it is ever abandoned. The sender is told plainly
that the message went unanswered and should be resent, because silence after a
promised reply is the worst available outcome. The panel is then torn down; the
interactive session is restored only when nothing is queued, since queued work
claims the panel next anyway.

The check runs *after* the completion polls, so an answer that lands just past
the deadline still wins.

## The panel belongs to the sender while it is answering them

This main-panel input lock is retained as historical rationale. Receiver work
now owns a background tab, so local input remains routed to the user's selected
tab and never enters the receiver PTY.

Receiver dispatch focuses the brain panel, and a panel with a message in flight
is not "warm", so `leave_warm_receiver_for_interactive_input` did not fire and
local keystrokes were forwarded straight into the remote conversation's PTY.
They landed in the composer beside the injected prompt, and a local `Enter`
submitted it half-written.

While a remote turn is in flight, local keystrokes are dropped and the footer
says why. The interrupt key is deliberately exempt: a lock with no exit turns a
wedged remote turn into a trapped TUI, and Ctrl+C is how the user takes their
own agent back. That, plus the abandon deadline above, means the lock always
ends — by answer, by interrupt, or by timeout.

## A hook command may not depend on the working directory

This section records the earlier `.claude/brain-hooks/` repair. The later
workspace-lifecycle migration at the end of this document supersedes its exact
paths and removes the home-directory fallback; the root-anchoring principle
remains current.

Brain registered Claude's lifecycle hooks as `python3 .claude/brain-hooks/<script>.py`,
relative to the workspace root, on the documented assumption that "Claude runs
project hooks from the selected workspace." That assumption was wrong. Claude
runs a hook in the session's **current** working directory, and its Bash tool's
`cd` persists for the rest of the session. So the moment an agent ran something
like `cd ~/brain/projects && …`, the turn-complete hook could no longer be
found.

The failure mode is the expensive part. The hook is what writes the completion
artifact brain polls for, so its failure is silent and total: the agent answers
correctly, on screen, and the answer is never delivered. `receiver_started`
stays set, so the turn never ends, the queue never advances, the panel stays
locked, and the sender gets nothing. A missing file at a path that "looks
right" cost a delivered reply.

The command is now
`python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/<script>.py"`.
`CLAUDE_PROJECT_DIR` is what Claude exports for precisely this problem, and it
is correct even for a session Brain did not launch; `BRAIN_ROOT` covers the
launched case; `$HOME/brain` is the last resort. Verified by probing a real
session: with the agent cd'd into a subdirectory, the hook's cwd was that
subdirectory while `CLAUDE_PROJECT_DIR` still named the project root.

**Why not an absolute path.** `.claude/settings.json` lives inside the synced
workspace and is read on every machine, so `/Users/pablo/brain/...` would break
the machine whose root is `/Users/someone-else/brain`. That portability is what
motivated the relative path originally; the variable keeps it while dropping the
working-directory dependency. Codex already used this shape for the same reason.

The general rule: **a path recorded for later execution must be anchored to
something the recorder controls** — an exported root variable — never to
ambient state a tool call can change out from under it.

## A deadline alone cannot tell a stalled turn from a slow one

The abandon watchdog first shipped as a bare ten-minute deadline, which forced a
bad trade: long enough not to kill a genuinely slow answer meant long enough to
strand a queue behind a wedged one. Both failure modes are just "no completion
artifact yet", so no single duration separates them.

What separates them is whether anything is happening. Every frontend renders
*something* while it works — a spinner, an elapsed counter, streaming tool
output — so a panel that has not changed at all in ninety seconds is waiting on a
person, not on a model. Abandoning now needs both: five minutes open **and** a
completely quiet panel. A turn that keeps rendering is never abandoned, however
long it runs, and the deadline could drop from ten minutes to five precisely
because it no longer has to be generous enough to cover slow work.

The activity signal is deliberately the **panel**, not a per-frontend transcript
or session file. All three frontends draw into the same PTY, so reading the
screen is one implementation that is correct for all of them by construction,
rather than three that can drift — and a fourth frontend gets it for free. It
also measures the right thing: not whether a file grew, but whether the agent is
doing anything a person would recognise as work.

The asymmetry of the two errors sets the constant. Calling a working turn stalled
kills a good answer and tells the sender to resend something that was about to
arrive; waiting another minute on a truly wedged one costs a minute. So ninety
seconds is generous on purpose.

An abandoned message's sender is told it could not be processed and asked to
retry, on the channel it arrived on. Silence after a promised reply is the worst
available outcome, and it is the one the sender cannot distinguish from being
ignored.

## An empty 404 to the provider, a specific one to the log

An inbound message whose destination matches no workspace is answered with an
empty 404, so whoever probed the URL learns nothing about which addresses this
machine serves. That is the right answer to a stranger and the wrong one to the
owner: when a real message bounces because a configured address is stale, the
only visible evidence is a bare "404" in a provider dashboard, and the value that
needs changing is the one value the operator cannot see.

So the two audiences are separated. The provider still gets `Response::empty(404)`
— unchanged, and the reason the response body is not where this belongs. The
local log gets the address that arrived, every address this machine has
configured for that channel, and the `brain env set` command that fixes it. A
mismatch is then obvious on sight (`brain@new.example` arrived, this machine
publishes `brain@old.example`) instead of requiring the operator to guess which
of the two ends is wrong.

The logged destination is attacker-supplied and unverified at that point, so it
is stripped of control characters and truncated: an unrouted request must not be
able to forge log lines or flood the log. A payload that names no destination at
all is reported as its own fault rather than as a configuration mismatch, since
nothing about the configuration is wrong in that case.

## Two Resend keys, because one key cannot hold two permissions

Retrieving an inbound email needs a Resend key with read access; sending a reply
needs one with send access. A single full-access key satisfies both, and that is
what brain assumed. But a full-access key used for sending reportedly fans every
outbound event out to *every* webhook on the account, not just the domain the
workspace cares about — so the natural fix is a sending-only key, which then
cannot read inbound mail. The two requirements pull the single key in opposite
directions and no value satisfies both.

So the capability that needs the stronger permission gets its own credential.
`resend_full_access_api_key` is consulted for retrieval and attachment refresh;
`resend_sending_api_key` sends. Both are required and neither falls back to the
other: a fallback would let a workspace look configured while one of the two
capabilities was silently using a key that cannot perform it, which is exactly
the failure that is invisible until an email stops arriving.

The names carry the scope on purpose. `resend_full_access_api_key` is the one
place a full-access credential is wanted, and saying so in the name means a
reader never has to look up which of the two is allowed to be narrower.

This is least privilege arriving through the front door: the key that reads a
mailbox and the key that sends as a domain were only ever the same value for
convenience, and the webhook fan-out is what made the convenience cost visible.
The rename from `resend_api_key` is deliberately breaking — the email channel
reports `incomplete` until both are set, which is louder and safer than carrying
one key forward into a role it may not have permission for.

A refused retrieval now logs the HTTP status with its likely cause, because
401/403 (a key that cannot read) and 404 (a key from a different account) need
completely different fixes and are indistinguishable from the provider-facing
502 that a bare upstream failure produces.

## A transaction journal is machine-local, even though it lives in a synced root

Brain's multi-file writes (portable users, triage habits, task schema) stage a
journal plus per-file backups *beside* the live files, which puts them inside the
synced workspace root. That was originally described as a feature: another
machine could see the journal and recognize an interrupted publication.

It is not a feature, because recovery is a **rollback**, and a rollback is only
ever correct on the machine that crashed. The journal says "restore these live
files from these backups", and those backups hold that machine's pre-edit bytes.
Transferred to a peer, the same journal makes the peer overwrite its own live
`users.json` with a stranger's older generation and then push the result, so one
interrupted `brain user` edit on one laptop silently reverts the roster for the
whole workspace. If the backups do not arrive with it, recovery instead fails
hard and every load, including inbound receiver requests, reports users
unavailable until someone deletes the file by hand. A journal is the one artifact
whose meaning does not survive leaving its machine.

So the artifact families are sync excludes, in both the bisync argv and the
one-way push: `.brain-*` (every journal plus its staged/backup/restore scratch),
`*.brain-triage-*` (the siblings named after the live file, like
`.tasks.csv.brain-triage-<id>-0.staged`), and `*.transaction.lock` (per-machine
flocks with nothing portable in them). `watch::is_watch_relevant` mirrors them:
mid-transaction is the worst moment to fire a push, since the only thing such a
push could carry is a half-applied group.

Changing the filter set makes rclone bisync demand `--resync`, which brain already
handles: the run aborts with `PriorListingMissing` and `should_auto_resync` retries
once with a fresh baseline. Nothing is lost, because the newly excluded paths are
scratch by definition. Objects a previous version already uploaded stay on the
remote as inert junk; `rclone delete` on the remote path removes them.

## Managed lifecycle hooks belong to workspaces, and migrations are invisible

Global lifecycle hooks made one workspace's Brain integration visible to every
agent session a user started. Codex also needed a global hook command to infer a
root from ambient environment. That is broader than the behavior Brain owns:
the lifecycle bridge is meaningful only inside a configured Brain workspace.

The current contract installs `agent_session_start_hook.py` and
`agent_session_stop_hook.py` under each existing configured root's
`.brain/hooks/` directory. Claude and Codex keep their hook registration in that
same workspace; OpenCode keeps its thin plugin there as before. Codex requires
project hook trust, so a Codex process launched through Brain carries the
explicit bypass intended for automation that vets its hook sources. Brain owns
and byte-verifies those scripts. The tradeoff is visible: unrelated enabled
project hooks share that bypass, so adding one to a Brain workspace requires the
same review as adding executable project configuration.

Python remains the bridge runtime. All three frontends can execute other
commands, but the existing standard-library bridge already supplies JSON,
SQLite, locking, and atomic publication without a second compiled artifact or
installer path. OpenCode would still need its JavaScript event adapter. Python 3
is treated as an explicit prerequisite and checked by the installers.

Migration cannot be a release-note chore. Every command except help and version
runs the ordered machine migration table before ordinary dispatch. Each entry
has an `up` and `down` operation, removes superseded state, transforms retained
state, and creates missing target state. Current entries reconcile even after
their version stamp matches, because managed artifacts can be deleted or
rewritten after an upgrade. Workspace-local lifecycle directories remain setup
metadata for empty-workspace detection, so reconciliation before bootstrap does
not suppress first-run PARA and task seeding. An ordinary startup records its
version stamp best-effort: if that machine-local directory is read-only, the
idempotent reconciliation repeats later instead of masking the command's own
diagnostic. The installer still requires its explicit migration and stamp to
succeed. The main installer compares binary versions: the
new binary migrates an upgrade after replacement, while the still-installed
newer binary migrates a downgrade before replacement. This keeps the user out of
the migration protocol in both directions.

Removing a registration does not update an agent process that already read it.
The 0.71 migration therefore removes the global registrations immediately but
replaces the legacy workspace script paths with forwarding shims. A frontend
started before the upgrade can finish its session through the new generic
workspace hook instead of failing on a deleted file. No current frontend
setting points at these compatibility paths, and a later migration may remove
them once the compatibility window is no longer needed.

A skill sync reconciles rather than accumulates, and it recognizes its own
output by a marker rather than by a manifest. Rendered skills share
`.agents/skills` with skills the user writes there by hand, so an installer that
only ever adds cannot tell a deleted plugin's leftover copy from a hand-written
skill: the leftover is re-adopted as user-authored and relinked forever. Every
rendered directory therefore carries a `.brain-rendered` marker file, and a sync
deletes marked directories it no longer produces. A marker inside the directory
was chosen over a manifest in the state DB because it survives a cache wipe,
needs no extra path plumbing for the `--root` sandbox or the cross-workspace
migration, and fails safe in the one direction that matters: anything brain did
not render is unmarked, so the worst outcome of a lost marker is a leftover that
stays, never a user's skill that is deleted. The same asymmetry rules the
frontend sweep, which removes only symlinks that point into `.agents/skills` at
a target that no longer exists.
