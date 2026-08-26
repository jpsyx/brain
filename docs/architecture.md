# Architecture

`brain` is a small Rust CLI that browses one selected PARA-organized workspace
(projects / areas / resources) and acts as the single terminal entry point for
the user's knowledge and task workflows.

As of the tasks↔brain merge, `brain` is the single CLI for both the second
brain and the task system; the standalone `tasks` binary is gone. Its two
execution surfaces are a persistent TUI and short-lived command families:

- **Bare `brain`** (and interactive `brain tasks …` routes) opens a
  **persistent shell**
  (`tui/`) with **three main views**: the **tasks view** (task management,
  agenda, triage; the startup default) and the **brain-directory search view**
  (fuzzy-pick over the selected root), and the **logs view** (scrollable
  diagnostics), plus one app-level **brain panel** (an
  interactive agent session in a PTY). You switch main views with
  `Ctrl+L`/`Ctrl+H` (cycle) or `Ctrl+T`/`Ctrl+B` (jump); the brain panel
  persists across a switch and closing it makes the main view full-width. The
  process owns the terminal until you quit and keeps UUID-scoped SQLite state
  for frontend sessions, completion delivery, and panel layout. Claude may
  resume an eligible transcript, OpenCode may resume an eligible live root
  session from the exact selected workspace, and Codex may resume when its
  exact rollout remains on disk. Each frontend starts fresh when its evidence
  is missing.
  See
  [glossary.md](glossary.md) for the main-view / sub-view / panel vocabulary.
- **Short-lived command families** cover non-TUI task utilities, config, env,
  workspace, portable users, sync, personalization, skills, server/receiver,
  habits, checks, and reindexing. They mutate or report through their focused handlers, then
  exit without opening the persistent shell. `brain tasks complete`,
  `brain tasks add`, `brain tasks set`, `brain tasks doctor`, and
  `brain tasks --no-tui` are short-lived; `brain tasks search` opens the
  persistent TUI.

There are **no** shell-mutating one-shot commands: no `cd`, `msg`, or
per-bucket search subcommand, and no freeform note search. Interactive
file-open, Finder-reveal, PDF, trash, and agent-launch actions happen inside
the persistent shell by spawning processes. The binary therefore needs no
parent-shell cooperation, wrapper, or plan protocol.

## One binary, run directly

```
user types `brain …`
  └─→ run.sh
       ├─ rebuilds the binary if any src/*.rs (or Cargo.toml) is newer
       │    (cargo build --release; build chatter → stderr)
       └─ exec target/release/brain "$@"   (forwards every argument)

the binary:
  ├─ ordinary run → writes a timestamped `/tmp` log; `--verbose` mirrors logs to stdout
  ├─ server/receiver status, receiver url, receiver details/email/phone → literal
  │    read-only probe, no run log or repair
  ├─ help / version → print and exit without opening the TUI
  ├─ tasks complete / doctor / --no-tui → mutation, health check, or plain output
  ├─ tasks search → opens the persistent TUI with a custom task search
  ├─ config / env / workspace / user → selected configuration or registry management
  ├─ sync / personalization / skills → focused setup, reporting, or mutation
  ├─ server / receiver / habits / checks / reindexing → focused handlers
  └─ bare brain and other interactive task routes → persistent TUI on /dev/tty
```

Help and version exit without opening the TUI. After these explicit exits,
bare `brain` and interactive task routes open the persistent TUI.

The intentional stdout families are `config/env/version`, `workspace list`,
`receiver` details and `receiver email` / `receiver phone` addresses,
explicit plain-task output, and help. `--verbose` mirrors logs to stdout for
non-TUI commands. Clap errors and diagnostics go to stderr. The TUI renders to
`/dev/tty`; TUI runs keep stdout quiet and expose the log through the tasks
command palette. The binary
opens files, cds its own PTY, launches the selected agent frontend, and reveals in Finder itself,
from inside the running shell. See [decisions.md](decisions.md) for why Brain
needs no wrapper or plan protocol, and [integrations.md](integrations.md) for
the launch/handoff detail.

## High-level data flow (inside the binary)

```
argv
 └─→ Cli::parse                          (cli/)
      ├─→ help / -v / --version / Cmd::Version
      │    └─→ print and exit before workspace bootstrap or command gates
      ├─→ startup_migration::run_current
      │    └─→ migrate by version, clean obsolete machine state, and reconcile
      │         managed artifacts in every existing configured workspace
      ├─→ status classifier              (server/receiver status and the receiver
      │                                   details/address routes skip write boundaries)
      ├─→ logging::init                  (other commands: `/tmp` log; optional stdout mirror)
      ├─→ workspace::bootstrap            (explicit per-invocation policy)
      │    ├─ context-free/internal → no registry, root, or prompt
      │    ├─ create/attach/remove/repair → registry capability only
      │    ├─ receiver status/url/details/email/phone → read-only selected context,
      │    │    no bootstrap-time readiness repair, users transaction recovery,
      │    │    or skills render
      │    └─ ordinary command → migrate only without a valid v2 registry,
      │         select once, validate readiness, repair interactively,
      │         return CommandContext
      └─→ command::dispatch::run          (focused handlers under command/)
           ├─→ configuration / sync / server / workspace / reindex handlers
           └─→ settings::ensure_markdown_to_pdf (tasks/TUI prerequisite gate)
           ├─ no subcommand ─────────→ tasks_launch(default view) → tui::run_tui (MERGED SHELL, tasks view)
           └─ Cmd::Tasks(rest)       ─→ TasksCli::parse_from(rest) → tasks_launch:
                                          complete → complete::run (native CSV completion,
                                                       then tasks::agenda re-syncs the day's agenda)
                                          sync-agenda → command::tasks::sync_agenda (agenda sync alone)
                                          add      → add::create_in_workspace
                                          set      → set::set_in_workspace (pure set::plan decides)
                                          doctor   → doctor::run_doctor
                                          --no-tui → plain::print_plain
                                          search   → CustomSearch → tui::run_tui
                                          else     → tui::run_tui (MERGED SHELL)

Empty-workspace startup is handled at `command::tasks::prepare_empty_workspace`.
It waits for a configured pull when the selected root contains only setup
metadata, initializes the portable config, task stores, lookup CSVs, counters,
and PARA directories, then publishes the initialized tree with a configured
push before the task CSVs are loaded.

tui::run_tui(TuiLaunch) (thin persistent-shell facade)
 ├─→ command::tasks::browse converts the task clap DTO once into owned
 │   TaskViewOptions, then moves the resolved view, task rows, selector state,
 │   and workspace context into TuiLaunch
 └─→ TuiRuntime::start(TuiLaunch)
      ├─→ named startup stages own singleton, receiver endpoint, server lease,
      │   terminal, App/session state, watcher, and periodic puller
      ├─→ command_context.workspace.root()   (immutable selected root snapshot)
      ├─→ build_search(brain_root)            (entry::collect over all buckets → picker::App)
      └─→ tick → draw → poll/read → application update
       ├─ agent::SessionStore: reap dead locks, scoped resume / claim or register
       ├─ BrainPanelState owns one AgentController per live main or ephemeral tab
       │    ├─ access::AccessPolicy snapshots trusted portable mode/root/actor
       │    ├─ agent::{ClaudeFrontend,CodexFrontend,OpenCodeFrontend} translate semantic operations
       │    ├─ agent::registry owns construction, lifecycle, health, and compatibility metadata
       │    └─ PtyPane clears inherited env, spawns the complete spec, and carries bytes
       ├─ Ctrl+L/H cycle views, Ctrl+T/B jump; Alt+H/L switch panel focus
       ├─ Ctrl+P opens a contextual command palette backed by tui::palette::CommandPalette
       │    ├─ task/log rows wrap app commands (including habits and agenda) in GlobalAction
       │    │  and keep only task-specific operations in TaskAction
       │    ├─ search rows carry feature-owned SearchAction values
       │    └─ every app command wraps one closed GlobalAction and runs through App::execute_global_action
       ├─ Enter on a file opens it in place (open_target spawners) — shell stays up
       └─ quit → the loop just returns (no plan, no wrapper handoff)
```

The task and brain-search views share the pure picker logic (`picker::App` matching /
navigation, rendered via `picker::draw_into`) and the `menu` palette in the
search view. The search panel lives in a bordered half of the shell alongside
the live brain panel; opening a file or rescoping a bucket happens in place
and the shell stays up.

## Test module structure

Large test suites keep their shared fixture scope in a parent `tests.rs` or
integration-test entry point, then `include!` cohesive sections from a sibling
`*_sections/` directory. Section filenames name the behavior, subsystem, or
invariant they cover, such as `remote_schema_preflight.rs` or
`receiver_origin_upgrade.rs`; they never encode split order. The
`tests/module_structure.rs` architecture guard checks every tracked Rust file
under `src/` and `tests/` and fails with each offending path if a numbered
`part_<digits>.rs` fragment returns.

## Module ownership boundaries

Large command families are organized as directory modules whose children mirror
their dependency seams. In particular, `sync::command` keeps orchestration in
`command/mod.rs`, while status formatting, direction decisions, conflict
presentation, and file-finding rendering live under `command/reporting/`; the
parent re-exports the stable call paths. The same coordinator-versus-detail
rule applies across the large runtime families:

| Boundary | Coordinator / public seam | Focused implementation modules |
| --- | --- | --- |
| Agent compatibility | `agent/{claude,opencode}/probe.rs` | `agent/command_probe.rs` owns shared bounded subprocess execution; each frontend probe owns policy, caching, and characterization |
| Ordinary task commands | `command/tasks.rs` | `tasks/set.rs` owns field mutation; `tasks/browse.rs` owns query resolution and browser launch |
| Receiver installation | `command/server/receiver/{hooks,setup}.rs` | `hooks/{artifact,json}.rs` own confined artifacts and atomic JSON; `setup/validation.rs` owns pure input validation |
| Workspace startup | `workspace/{bootstrap,initialize}.rs` | `bootstrap/selection.rs` owns selector precedence; `initialize/seed.rs` owns empty-workspace detection and seeding |
| Shared HTTP server | `server/mod.rs` | `server/request.rs` owns request dispatch; `workspace_route/loader.rs` owns verified context loading; `lifecycle/table/mutation.rs` owns lease mutations; `receiver/http/email/fetch.rs` owns Resend retrieval and parsing |
| Durable receiver state | `state/receiver/` | `identity.rs` owns logical conversation identity; `job_state.rs` owns lifecycle transitions; `recovery_policy.rs` owns the clock-injected pure lease decision; `schema.rs` owns the receiver schema coordinator, `schema/recovery.rs` owns schema-v10 columns, repair, and v10-to-v9 downgrade mapping, and `schema/token.rs` owns semantic UUID token reconciliation; `store.rs` owns acceptance and conversation mutations; `store/load.rs` owns typed row decoding; `store/observation.rs` owns exact token, owner, instance, live-session, revision, and lifecycle-deadline commits; `store/completion.rs` atomically keeps first lifecycle facts separate from the current-attempt cursor while binding the exact completed session and terminal job state; `store/reconciliation.rs` owns the immediate oldest-blocker transaction and neutral semantic effects, with focused candidate, cleanup-acknowledgement, and terminal helpers beneath it; `store/claim/live.rs` owns the workspace-wide live-claim exclusion shared by every claim transaction; `store/claim/next.rs` owns ordinary FIFO selection and refuses due recovery attempts; `store/claim/recovery.rs` gates recovery on the globally oldest claimable or blocking row, then claims only an ownerless, cleanup-acknowledged recovery already persisted by reconciliation; `store/claim.rs` owns launch CAS, transitions, and bounded ordinary failures that remain provably pre-spawn; `store/session.rs` owns exact receiver registration, release, and lifecycle binding attribution |
| Sync | `sync/{csv_sync,identity,setup}.rs` and `sync/command/reporting.rs` | `csv_sync/transport.rs`, `identity/probe.rs`, `setup/prompt.rs`, and `command/reporting/findings.rs` isolate external transport, probing, terminal input, and formatting |
| TUI | `tui/runtime/mod.rs` and focused state/coordinator modules | `runtime/builder.rs` owns ordered startup acquisition and application assembly; `runtime/mod.rs` owns process-lifetime execution and resources; `state/tasks.rs` owns task-list view, query, selection, and layout state; `state/shell.rs` owns main-view, focus, search, logs, layout, and active-tab navigation; `runtime/tick.rs` owns the sole recurring receiver-consumer call; `runtime/shutdown.rs` pins acquisition and teardown state; `runtime/terminal.rs` owns `/dev/tty`, ratatui, and terminal-mode restoration; `receiver/runtime.rs` owns receiver intent, freshness, and the durable-run handle; `app_brain/launch/session.rs`, `app_actions/triage/decision.rs`, and `palette/command/catalog.rs` isolate session launch, pure triage decisions, and the command catalog |
| Live receiver runtime | `tui/receiver/{planning,runtime,run,session,failure,attachments}.rs` and `tui/app_brain/receiver/` | `planning.rs` renders a frontend-neutral launch plan with the exact terminal job-token marker from an already-authorized session choice; `session.rs` owns isolated hook identity plus fresh/resume registration guards; `failure.rs` owns pre-spawn controller/session/claim cleanup; `run.rs` owns durable local-run state; `attachments.rs` owns the bounded background staging worker, exact generation coordinator, and owning batch guard; `app_brain/receiver/dispatch.rs` owns FIFO claim, freshness, and durable controls; `resume.rs` owns binding selection, native-history validation, exact resume registration, and the typed fresh/lost/deferred decision after fresh owner checks; `launch.rs` owns capability checks, fresh registration, receiver-only observation authority, launch planning, and launch preparation; `launch_effects.rs` owns controller spawn, background-tab allocation, and the exact durable `launched` boundary; `ownership.rs` owns fresh-clock exact-owner renewal and pre-spawn retry decisions; `attachment_dispatch.rs` owns nonblocking staging-result decisions; `active.rs` owns exact-claim renewal, exact-tab lifecycle polling, and fresh-time atomic observation commits, while `active/terminal.rs` owns exact terminal authorization, effects, and local cleanup; `cleanup.rs` owns exact-instance response, snapshot, and lock removal; `diagnostic.rs` owns the stable content-free observation log shape; `shutdown.rs` owns receiver-first local teardown without replaying `launched` work; `artifact.rs` owns exact token-bound completion correlation while private final text remains separate from lifecycle evidence; `reply.rs` preserves immutable provider delivery. No in-memory or socket consumer remains; `ReceiverRuntime` holds only a narrowly named legacy endpoint lifetime for BR-18 builder compatibility. |
| Structured env | `env/vars/mod.rs` | `env/vars/path.rs` owns dotted-path traversal and flattening |

Session-store persistence is colocated under `state/session_store.rs`, and sync
identity's external command adapter lives under
`sync/identity/remote_command.rs`. Large tests follow the same rule: behavior
groups live in focused child modules but remain descendants of the production
module, preserving access to private implementation details without making a
production file a test-suite container. The line-count audit treats production,
unit tests, integration tests, and fixtures alike.

## Multi-workspace foundation boundary

The foundation separates workspace payload and runtime state along these
boundaries:

| Owner | Location | Examples |
| --- | --- | --- |
| Portable workspace | `<workspace-root>/` | Notes, tasks, `.config/workspace.json`, `.config/users.json`, config, personalization, extensions, and plugins |
| Machine registry | `$XDG_CONFIG_HOME/brain/env.json` (fallback `~/.config/brain/env.json`) | Schema-v2 default plus each canonical record's UUID, root, aliases, local user, receiver switch, and siloed env object |
| Workspace runtime | `~/.cache/brain/workspaces/<workspace-uuid>/` | State DB, TUI/task/user locks, inbox, responses, capability artifacts, migration journal/backups, and sync lock/journal/current state/workdir/baselines |
| Shared infrastructure | `~/.cache/brain/server/` | Generation-tagged process coordination and an infrastructure-only log, never a default workspace payload path |

Each workspace state DB now contains the durable receiver foundation. An
authenticated `InboundJob` can be inserted with its logical conversation in
one transaction, then claimed in FIFO order with an expiring owner lease
without deleting the row. Explicit state and retry metadata survive database
reopen, and one workspace can have at most one unexpired receiver owner. The
narrow `AppServices` launch surface atomically claims and loads the immutable
job/conversation pair, moves only an exact live launch-eligible owner to
`launching`, and records content-free bounded pre-acceptance retries.
Conversation rows store Brain-owned markdown plus the current
frontend/native-session binding. Native resume is valid only for that same
frontend; another frontend starts fresh from the portable transcript.
Receiver-session registration rows preserve the exact workspace, logical
conversation, actor, channel, frontend, remote instance, registered launch ID,
and lifecycle-reported actual native ID. A fresh Claude run may use its
registered ID as the native ID; fresh Codex and OpenCode runs must rotate their
placeholder. A resumed run instead registers the conversation's already-bound
native ID, so every frontend may confirm that same ID when the exact durable
conversation binding proves the resume lineage.
Registration and session ownership are created in one transaction, and binding
replacement must match that complete durable tuple.
The durable reconciler inspects the oldest blocking nonterminal row under one
immediate transaction before ordinary work may advance. It compares the full
token, owner, retry, observation, attempt, and deadline snapshot. An unaccepted
timeout becomes a bounded ordinary retry; a first accepted stall becomes one
ownerless due recovery with the exact native binding preserved and an exact
instance/session cleanup fence. A recovery claim remains blocked until Task 4
shuts down that superseded native run and acknowledges its exact fence. Both
targeted and discovery claims also require the recovery to be the workspace's
globally oldest claimable or blocking row. Exhaustion, ownerless recovery or
absolute expiry, missing resume evidence, incomplete legacy completion, and
any claimed-recovery planning, registration, spawn, or shutdown failure become
terminal with a pending unavailable-notice intent. The transaction returns
content-free effect identifiers, while controller cleanup, validation, launch,
and notice delivery remain outside the state layer.
`app_brain/receiver/resume.rs` treats that binding only as a candidate. It asks
the selected `AgentController` to validate the native history, then renews the
exact durable owner before interpreting missing history or a validation error
as a Fresh fallback. A validated candidate is claimed through its exact session
registration and followed by another renewal before a rejected claim may fall
back Fresh. Lost and deferred ownership are separate typed outcomes and can
never become Fresh. `tui::receiver::planning` renders that already-authorized
choice with a UTF-8-safe recovery prompt capped at 47 KiB. The isolated-tab coordinator
uses these operations from the one recurring `App::tick_receiver` call.
`app_brain/receiver/dispatch.rs` owns claim, freshness, and durable controls;
`resume.rs` owns resume validation and registration; `launch.rs` owns capability
checks, fresh registration, planning, and launch preparation;
`launch_effects.rs` owns controller spawn and background insertion; and
`ownership.rs` owns the semantic fresh-clock exact-owner gates used after slow
effects and before retry mutation. `attachment_dispatch.rs` owns nonblocking
staging-result decisions; `active.rs` owns exact-claim renewal and polling,
while `active/terminal.rs` owns terminal authorization and cleanup;
`artifact.rs` owns content-free exact completion correlation;
and `reply.rs` preserves immutable provider delivery. No receiver branch injects
the main panel.

Authenticated provider ingress now uses this foundation as its acceptance
boundary. After final workspace and lease authority revalidation, the shared
server opens the addressed workspace DB with the remaining HTTP handoff budget
and atomically inserts or deduplicates the exact job and conversation. Provider
success follows that transaction, so process or TUI restart cannot erase
accepted work. The shared process still requires a live enabled lease for
routing and remains ingress-only. The live TUI alone claims durable work and
owns agent launch, completion, and delivery.

One bootstrap resolves an immutable `CommandContext` / `WorkspaceContext`.
Env, config, personalization, state, TUI, tasks, reindex, sync, and child
integrations consume that selection or a path derived from it. Ordinary
runtime code does not reopen the registry or resolve a global root. Detached
workspace-owned children carry `--workspace <canonical-name>` plus the selected
UUID in `BRAIN_WORKSPACE_ID`; bootstrap refuses the child if that expected UUID
does not match the selected registry record. Integrations receive
`BRAIN_WORKSPACE_ID`, `BRAIN_WORKSPACE`, `BRAIN_ROOT`, `BRAIN_ACTOR_ID`, and
`BRAIN_CHANNEL`, with agent-session values added separately.

Active run logs remain under `/tmp` through `logging.rs`.
`WorkspacePaths::logs_dir` is reserved and unused; current diagnostic logs do
not use that UUID-scoped path.

The frontend-neutral `agent` facade, concrete Claude/Codex/OpenCode adapters,
registry-driven construction and lifecycle metadata, PTY transport, main and
triage controller ownership, receiver controller dispatch, advisory portable
access modes, and the OpenCode lifecycle plugin are active. Coordinated
task-schema activation is available only through explicit workspace migration.
The shared process control protocol, live TUI leases,
heartbeats, crash recovery, final-TUI shutdown, opaque-ingress routing,
authentication, actor resolution, durable job admission, TUI durable dispatch,
and delivery are active.
`workspace_only` is easy-to-bypass prompt guidance plus capability filtering,
not a security or isolation boundary. It reduces accidents and naive leakage
among trusted users; adversarial or sensitive workloads require an external
OS, VM, machine, or container boundary. Changing the machine default never
changes portable access mode. The controller accepts a workspace-only launch
only when its capability plan has the same access mode and selected workspace
UUID; unrestricted launches carry no plan and do not parse capability
configuration. Those are the only accepted access contexts. Unrestricted with
any plan, workspace-only without a plan, and mismatched workspace-only plans
all fail before frontend or transport work.

## Modules

### `main.rs`
The binary entry point is intentionally thin: parse, context-free help/version
exit, startup migration, logging, one workspace bootstrap, one dispatch call,
and one top-level error
boundary. It links the library modules instead of declaring a duplicate module
tree. `command/dispatch.rs` owns the exhaustive `Cmd` routing, while focused
`command/{configuration,tasks,sync,server,workspace,users,reindex}` modules own the
existing handlers. `command/server/` further separates receiver setup, HTTP
server lifecycle, habits dispatch, and machine-wide process cleanup. Receiver command ownership is reflected
on disk: `receiver/mod.rs` owns dispatch, `receiver/setup/` owns selected-record
provider planning plus portable-user mapping, `receiver/url.rs` owns the webhook
URLs, `receiver/identity.rs` owns the configured per-channel address behind
`receiver email` / `receiver phone`, and `receiver/details.rs` owns the bare
`brain receiver` listing (including the shared intent-and-liveness rows
`receiver status` prints). The listing builds a read-only context per registered
record through `workspace::peer_context`, the same helper `workspace list` uses,
so one unreadable peer degrades to a themed note instead of failing the run. Its `setup/transaction.rs` owns
bounded rollback orchestration across the selected machine record, portable
users, and hook artifacts; `setup/transaction/{lock,snapshot}.rs` own the
advisory lock and exact filesystem restoration mechanics. One workspace-local advisory lock spans snapshot, every write,
commit, and rollback, so concurrent setup attempts cannot claim or restore one
another's identical after-images. `receiver/hooks.rs` owns
registry-driven frontend lifecycle installation;
its focused installer tests live in the owned `receiver/hooks/tests.rs`
submodule.

### `startup_migration/`

`startup_migration/mod.rs` owns a small ordered table of version boundaries.
Each migration supplies `up` and `down`; `version.rs` provides the three-part
semantic comparison, and a focused module such as `lifecycle.rs` owns the
actual cleanup, transformation, and target-state reconciliation. Ordinary
commands run toward the compiled version and then reconcile its migrations so
missing managed files self-heal. The lifecycle migration also retains inert
workspace-local forwarding shims for hook commands cached by agent processes
that started before the migration; no current frontend setting registers those
legacy paths. The 0.81 receiver-lifecycle migration installs the normalized
observation producers and has an exact down path that removes only its canonical
hooks and script while restoring the frozen 0.80 OpenCode plugin. Same-basename
user commands are not managed entries. The version stamp lives at
`$XDG_CONFIG_HOME/brain/migrations/version` (falling back to
`~/.config/brain/migrations/version`). Help and version exit before this module.

`install.sh` compares the installed and built versions. An upgrade installs the
new binary and asks it to migrate forward; a downgrade asks the still-installed
newer binary to migrate backward before replacement. The hidden `__migrate`
route exists only for that installer handoff and exits before ordinary
bootstrap.

### `cli/`
The clap derive surface, split by command family. `mod.rs` keeps the parser
entry, public re-exports, and the small top-level `Cmd`; `global`,
`configuration`, `tasks`, `sync`, `server`, `workspace`, and `users` own their focused
arguments. All former `crate::cli::*` type paths remain stable. `Cli` owns the
global flags, including raw `--workspace/-w <workspace>` selection, plus one
optional `Cmd`. The shared real/test parser normalization extracts that selector
before Clap's delegated `tasks` tail can capture it, including after a task
positional; `--` stops extraction. Clap retains the exact raw selector, and
registry-aware dispatch resolves it case-insensitively as a canonical name or
alias. Bare `brain` remains equivalent to `brain tasks`.

### `logging.rs`
Per-run logging. `logging::init` always creates a timestamped
mode-`0600` `/tmp/<rfc3339-nanos>.log` file, and `--verbose` mirrors log
lines to stdout for non-TUI commands, and prints the final log path at process
exit. Before the persistent shell takes over `/dev/tty`, `main.rs` disables the
stdout mirror; the TUI keeps the log path in `App` and offers receiver and brain
log actions in the command palette that switch the main panel to a log view.
the tasks command palette. Command handlers and thin IO shells call
`logging::log` at phase boundaries: dispatch, config/env/persona actions,
task CSV loads and writes, sync/rclone work, server lifecycle probes, doctor
checks, and skill installation. `main.rs` passes argv through the pure central
redactor before either file logging or verbose mirroring, so receiver provider
credentials and portable phone/email values never cross the log boundary.
Env assignment redaction delegates to the authoritative env sensitivity
classifier, including whole `agent_capabilities` values and nested MCP
credential fields, after the same case-and-dash canonicalization as the env
command.

### `paths.rs`
Legacy single-root resolution only. `brain_root()` / `brain_root_path()` retain
the pre-migration flat-root, read-only pointer, and `~/brain` fallback boundary
needed to construct the first registry record. Ordinary commands do not use
them to select a workspace; bootstrap supplies an immutable
`WorkspaceContext::root()`. The IO-free compatibility pieces
(`parse_brain_root_file`, `expand_tilde_with_home`) remain unit-testable without
a real `$HOME` or pointer file. See [config.md](config.md).

### `workspace/`
The typed, selection-independent workspace foundation. `id` owns the immutable
UUID newtype, `name` validates canonical lower-case slugs, `context` owns an
already-resolved root and the machine's local user ID, and `paths` derives every
machine-local runtime path from the immutable UUID. `registry/` owns the
versioned machine registry, split by responsibility into `model` (schema and
validated mutations), `validate` (pure whole-registry invariants), `select`
(borrowed canonical/default/alias resolution), `store` (the fixed registry
path, lock-owning transactions, and same-directory atomic replacement), `lock`
(the bounded SQLite transaction lock on the stable adjacent database),
and `migrate` (the one-time flat-env conversion and exact-byte backup).
`bootstrap_policy` classifies every invocation before workspace IO, while
`bootstrap` executes that policy. `read_only` owns the selected-workspace
status path and uses non-recovering readers so observation cannot create a
lock, config value, state DB, or skill render. Context-free and hidden internal-server
routes cannot prompt. Create, attach, remove, and repair first run the
`command::preflight` prompt-and-validation stage; only a complete request may
trigger legacy migration and receive a registry capability. Ordinary commands
inspect the fixed registry path and enter legacy migration only when it is not
already at the current schema; a valid registry avoids all legacy root/config lookup.
They then require a ready selected workspace. `readiness` is
the pure manifest/portable-membership/local-user decision,
`manifest` owns strict portable identity parsing, and successful bootstrap
returns one immutable `CommandContext` containing an `Arc<WorkspaceContext>`,
the request's resolved local `ActorContext`, and the registry store. Interactive repair uses injected `BufRead`/`Write`,
persists under the registry transaction, reloads, and continues the originally
requested command. Root-local stores take the context, machine-env writes also
take its exact `RegistryStore`, and the TUI retains the same `Arc` for watcher,
receiver, session, rendering, state, response, and sync paths. Brain-owned
children receive the typed workspace/actor integration environment.
Detached workspace-owned sync children additionally carry
`--workspace <canonical-name>` and an expected `BRAIN_WORKSPACE_ID` that bootstrap
checks against the selected registry UUID. The detached shared-server child is the deliberate
exception: it owns only machine-shared lifecycle/control state and resolves
request payloads by workspace UUID, so it has no selected `--workspace` argument.

`requirements/` is the centralized, read-only selected-workspace health
inspector. It keeps required availability (root, compatible manifest, portable
users, and local user selection) separate from optional feature health
(`off`, `ready`, or `incomplete`). Its focused inspectors cover sync, receiver
channels, access/capabilities, triage, PDF conversion, Linear, personalization,
and browser/web views; rendering exposes prompt secrecy and exact repair syntax
without stored values. Startup reuses only its required-field decision, so this
model does not replace or broaden the readiness state machine. Workspace list,
sync status, receiver status, and tasks doctor call it with the already-pinned
`CommandContext`; none consults a peer workspace or repairs state while
observing it.

### `actor/`

The immutable effective person for one request lineage. `resolve` uses the
machine's portable local user for interactive work, or an enabled normalized
phone/email identity after provider authentication. `context` carries the
validated person ID, display name, and initiating channel through queueing,
session lookup, hooks, task-agent prompts, completion, and response delivery.
When readiness admits a legacy workspace with no portable user file,
`local_actor` uses its exact lower-case kebab legacy local ID as an interactive
compatibility actor and writes nothing; readiness never activates portable
migration. Readiness rejects every nonblank ID that the `UserId` parser would
reject, so actor bootstrap cannot discover a weaker legacy acceptance rule.

### `agent/`

The frontend-neutral agent boundary. `controller` owns the semantic
`AgentController` facade and the transport trait, so callers can type, submit,
queue, start sessions, launch, inspect completion and session eligibility, snapshot,
observe bounded receiver lifecycle facts, and shut down without constructing
frontend keystrokes. `observation` owns the opaque cursor and neutral
phase/boundary/progress-pulse/request/result/error model, exact current-session ownership gates,
and the shared descriptor-bound schema-v1 snapshot validator. The controller
validates the trusted token, instance, canonical path, lifecycle session, actor,
workspace, channel, and selected frontend before delegation, then reopens
durable state and proves the same ownership again after the read. On Unix the
reader opens the cache path component by component without following symlinks,
validates the opened regular file before and after one bounded read, and parses
only those handle-bound bytes. Receiver coordination therefore never parses
provider transcripts, rollouts, event names, or snapshot grammar.
`BrainPanelState` provides the smallest exact-tab observation facade, while
`AppServices` loads durable cursor facts and applies one atomic receiver
observation batch, including a pulse that carries no new lifecycle phase. The
raw producer revision stays private to the opaque cursor and the state model's
agent-to-state conversion seam; receiver coordination cannot extract or name
it. The single-observation state API exposes only accepted and
progressing phases through `ReceiverNonterminalObservationPhase`; terminal
evidence can reach persistence only through the registration-aware batch
transaction. The active coordinator resolves the current lifecycle
session before crossing that facade and samples a fresh authorization time only
after evidence validation. A terminal lifecycle batch persists that exact
native session as the conversation binding in the same transaction that marks
the job done and clears the claim, so a binding failure rolls back the entire
completion. Local cleanup is deliberately outside that transaction and removes
only the completed or abandoned instance's response, snapshot, and sibling
lock. `frontend` defines the
crate-private frontend trait and adapter operation enum plus complete launch
request and launch spec types. Concrete Claude, Codex, and OpenCode adapters
are also crate-private; callers and black-box tests construct a controller and
cross only the facade. Public launch-spec and input-sequence values are the
transport DTOs needed by external `AgentTransport` implementations, while
their adapter-side constructors remain internal. `input`,
`session`, and `hooks` own the shared input, validated session-plan, completion,
and hook metadata values. `session` owns the canonical `AgentKind` identity,
frontend-neutral `SessionStore`, immutable `SessionScope`, and durable
`CompletionStatus`;
the crate-level `session.rs` re-exports it and keeps adapter-backed command/env
wrappers for compatibility callers and pure tests. `claude`, `codex`, and
`opencode` own launch syntax, input sequences, completion, transcript or
session-discovery, and lifecycle rules. `registry` is the exhaustive table of
frontend constructors, command metadata, lifecycle installations, exact health
checks, capability evidence, and compatibility probes. Shared command, doctor,
and setup code consume that table instead of switching on concrete frontends.
`PtyPane` implements `AgentTransport`. The main panel stores an
`Option<AgentController>` because it may be closed; every live ephemeral tab
owns one `AgentController` directly. Keyboard, receiver,
draw, scroll, close, and event-loop code call controller semantics and never
construct frontend keystrokes. Busy-turn follow-up is one controller operation;
each adapter returns the complete native text and final-key sequence.
`opencode` merges Brain's reserved inline configuration, performs isolated
feature and schema probes, discovers resumable sessions for the exact selected
root, and translates semantic controller actions to OpenCode's native input.
Its compatibility probe runs version, TUI-option, session-list, generated-config,
and plugin-load checks in disposable HOME/XDG roots. Claude's compatibility
probe uses that same isolated runner to require the `prompt_id` hook capability
represented by Claude Code 2.1.196 or later, recognizing only one exact
`major.minor.patch (Claude Code)` output record. `agent/command_probe.rs` owns
the bounded process-group runner and read-only command capture; each frontend's
`probe.rs` retains only compatibility policy and caching. Successful reports
are cached by configured command for the process; failed probes remain
actionable and are not cached as compatibility evidence. Session discovery runs
`session list --format json` in the selected root and admits only non-archived,
non-deleted root sessions whose reported directory resolves to that same root.
`LaunchRequest::HookMetadata` is trusted input that adapters merge into the
explicit child environment. The plan-mandated `LaunchSpec::hooks` slot is
currently reserved and empty; `PtyPane` does not consume a second hook channel.
Receiver launches add the exact durable job token and its UUID-scoped
observation path to that metadata. Main-panel and skill-session requests omit
both values. The registry installs one normalized observation bridge and
declares every managed event and exact source health check for Claude, Codex,
and OpenCode, so startup reconciliation replaces stale bridges without
discarding unrelated user hooks. Hook entries are managed by exact canonical or
explicitly known legacy command values, never by a basename shared with a user
script.

### `access/`

Portable access policy. `mode` owns the two stable config values; `prompt`
builds the trusted advisory text and deliberately naive literal outside-root
warning; `capabilities` snapshots mode plus prompt; `skills` resolves portable
logical names against the selected machine record; `enforcement` models honest
frontend evidence and levels; `artifact` owns symlink-safe validation and
removal below the selected workspace's trusted UUID cache root; `mcp` owns the
machine schema plus frontend runtime translation; `store` strictly
loads portable config, preserves unrelated keys, and
publishes mode changes through a synced same-directory atomic replacement. It
also validates or seeds the selected record before readiness, a new record
before publication, and every record when listing or explicitly migrating the
whole registry. Main, receiver, resumed, fresh, and triage launches all construct policy
from the selected workspace, resolved actor, and already-loaded portable
`Config`. Main and triage launch paths attach the same resolved capability plan.
The controller validates the plan's mode and credential provenance before a
frontend can render artifacts or reach the transport. Unrestricted launch
assembly bypasses portable and machine capability parsing, preserving normal
frontend pass-through even when unused capability data is malformed. TUI setup
implements the same distinction before `App` construction: access mode and live
settings remain strict, but unrestricted mode does not deserialize the unused
logical capability lists.
Inbound prompt text is not an input to policy construction.
The main-panel launch path also resolves the capability plan and adapter-owned
response identity before claiming a free resumable session. A later controller
launch failure releases the instance claim and clears the attempted response
identity.

### `users/`

The strict schema-1 portable people registry at
`<workspace-root>/.config/users.json`. `id` validates exact lower-case kebab
person IDs; `normalize` canonicalizes unambiguous phone numbers and
case-normalizes email addresses without provider-specific rewriting; `model`
and `validate` reject unknown schema fields and ambiguous enabled identities;
`store` publishes canonical JSON through a same-directory atomic replacement;
`transaction` coordinates grouped registry and assignment changes with a
portable recovery journal plus a workspace UUID-scoped machine lock; and
`command` owns pure add/update/remove mutations plus the inactive legacy
conversion proposal; and `assignment` holds the pure map from raw `assigned_to`
values to the members that replace them, shared by the migration cutover and
`brain user reassign`; and `select` holds the pure numbered-option helpers
(`local_user_choices`, `numbered_rows`, `interpret_row`) shared by the
`brain user` prompts and by readiness repair's "Who is this machine?" roster
picker, so both offer the same rows and read the same answers. A pending grouped transaction restores the old generation
before the next portable-user load. The selected machine record's
`local_user_id` must name one member when this portable file exists. It
identifies a person, not a device, owner, creator, or authorization principal.
The same ID may be selected on multiple machines for that same person; there is
no cross-machine identity split or audit identity.

`command/` owns the workspace CLI: `mutate` turns collected values into pure,
validated registry-only decisions and owns registry-only mutations;
`preflight` applies and validates registry-only prompt answers before migration;
`provision` owns create/attach filesystem and persistence transactions; `list`
renders deterministic themed output; `prompt` collects omitted human values
from `/dev/tty`; and `mod.rs` stays focused on dispatch and re-exports.
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
overlap after absolute lexical normalization. `RegistryStore::transaction`
acquires the interprocess lock before any load and holds it through mutation,
whole-candidate validation, atomic persistence, live-value replacement, and
create failure reporting. Lock timeout errors retain the lock path, owner PID when
available, and wait duration. The stable adjacent lock database is never
unlinked; the existing `rusqlite` dependency holds `BEGIN IMMEDIATE` for the
transaction lifetime. The lock database remains zero-length: it has no schema
or data writes and uses `journal_mode=OFF`, so locking needs neither
initialization nor journal files. This also lets registry persistence report
its own permissions failure in a read-only parent. SQLite releases its OS lock
on normal close or process exit. A stable owner sidecar supplies the diagnostic
PID. Atomic replacement remains a separate persistence guarantee.

When bootstrap policy permits registry access, `registry::migrate` checks this
fixed file before ordinary command dispatch. A current-schema registry is
returned without any write. Otherwise
it converts the legacy flat object into exactly one default record, resolving
the root from flat `root`, then the read-only legacy pointer, then
`<home>/brain`; the result is tilde-expanded and lexically normalized without
requiring the directory to exist. The new record receives no aliases,
an empty local-user placeholder, a de-duplicated receiver switch, and all other
machine-local flat values inside its `env`. It creates the root and the first
matching portable manifest before persisting the registry. If that root already
contains a valid portable manifest, migration adopts its workspace UUID and
receiver-ingress UUID without changing its bytes. Only a missing manifest
causes migration to generate those identities. Access policy is
deliberately not machine-local; readiness remains incomplete until this
machine's local user ID is supplied.

Before replacing an existing flat file, migration creates an adjacent
exact-byte `env.json.legacy-backup` (or the first free numeric suffix), then
uses the atomic registry store. Re-running sees the current schema and preserves both the
UUID and registry bytes without another backup. Registry-only create and attach
collect and validate their complete request before classifying the machine or
migrating. An existing
`env.json`, legacy `$XDG_CONFIG_HOME/brain-root` pointer, or `<home>/brain`
directory is legacy-install evidence and is migrated first. Only a genuinely
fresh machine with none of those sources lets the requested create/attach
become the first record. Workspace commands then load the registry explicitly and
never project through the compatibility view. `workspace create` creates only
its requested root after validating the complete registry candidate. It
tracks every missing root-directory component it creates. If later directory
creation or registry persistence fails, Brain never deletes those paths:
safe Rust 1.85 path APIs cannot atomically couple ownership verification with
deletion. A composed error keeps the original provisioning or persistence
error as its source and lists only the paths this invocation created, deepest
first, for manual inspection and cleanup. An `AlreadyExists` result is an
ownership race, not successful provisioning and is never added to that list.
`attach` requires an existing root with a strict compatible manifest and adopts
its UUID. Invalid manifests, duplicate UUIDs, and already-registered or
overlapping roots fail without changing registry bytes or root contents. Rename,
aliases, default changes, and removal use `RegistryStore` transactions. Adding
an alias already present on the same record is a typed error rather than a
successful no-op. Removal detaches only the record. Bootstrap resolves
`--workspace` once into `CommandContext`; env writes revalidate canonical name plus
UUID under the registry transaction, and all ordinary config, personalization,
task, reindex, sync, receiver, and TUI paths consume that same selected context.
Legacy root helpers remain only inside the tested one-time migration boundary.

Deserialization has a single trusted boundary: JSON first enters a private raw
schema DTO, then conversion runs the same pure whole-registry validator used by
mutations. Public `Deserialize<MachineRegistry>` uses that conversion, so a
successfully deserialized value is fully valid. `RegistryStore` parses the raw
DTO itself so structural JSON failures retain operation and path context while
domain failures retain their typed `RegistryError` variants. Storage failures
likewise retain their operation, primary and related paths, IO error kind, and
message.

After selection, bootstrap constructs one `WorkspaceContext` before passing it
to ordinary commands. Context fields are private and accessors expose
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
(get/set/resolve), `portable` (the three receiver-identity variables the
portable users roster superseded, plus the note and the `config set` refusal
they need), `render` (the `config list` table), and `markdown_pdf` (the
prerequisite). `vars` resolves a superseded variable from the roster before the
config store, so `brain config` reports the value brain enforces. See
[config.md](config.md).

### `env/`
The machine-local env store (the schema-v2 workspace registry at
`~/.config/brain/env.json`) and the `brain env` command. Same pure/impure split
as `settings/`: `store` (registry-scoped read/write of the *selected* record's
`env` map, rejecting structural keys at the save boundary), `schema` (the
declared `VARS`, `is_structural`, and `is_sensitive`), `vars` (get/set/resolve,
including the root-based `resolve_all_at` that resolves any workspace's rows from
its own root), `migrate` (legacy pointer/flat-env → first v2 record),
`breakdown` (assembling the whole-machine `brain env` view: machine-global rows
plus one `WorkspaceEnv` block per registered workspace plus the `VarDoc` legend;
`assemble` is pure, `collect` the registry-reading shell), and `render` (turning
a `Breakdown` into themed text; pure given a `Theme`). `brain env get`/`set`
stay scoped to the selected record while the list view spans the machine.
See [config.md](config.md) and [data-model.md](data-model.md).

### `personalization/`
The personalization store — content *about the workspace's people*, one persona
per portable user ID, at
`<brain-root>/.config/personalization.json` (beside the config store in
`settings::config_dir()`; it is just another brain config, inside the brain root
so it travels with the brain). Split into `persona` (one member's schema +
parse), `store` (path resolution in the brain config dir + load/save), `tags`
(the `TagStyle`/`TagStyles` model, the
generic defaults `mit`/`personal`/`work`, and pure label resolution with
raw-name fallback), `runtime` (explicit selected-workspace style loading for
the TUI's retained `App` state, from the local person's persona), `command` (the
`brain persona` show/list/get/set/edit logic — pure helpers + thin IO),
`workspace/templates.rs` (the `AGENTS.md` / `README.md` a new workspace is
seeded with, embedded from `templates/workspace/`),
`personas` (the user-ID-keyed store plus its schema-1 migration), and
`onboarding` (the skippable prompt, plus the pure decisions behind the
missing-persona gate every command runs at bootstrap). The task renderer's
`type_label` delegates here, so the public binary carries no personal taxonomy.
See [config.md](config.md) and [data-model.md](data-model.md).

### `skills/`
The brain skill pipeline (sub-project B): render the bundled skills into the
selected workspace's `.agents/skills` directory and fan them out to project
frontends (Claude, **Codex**, and OpenCode). Split into `model` (the shared `Skill`/`SkillFile` type), `embed` (the
`include_dir!`-embedded `skills/` dir → bundled `Skill`s), `plugin` (whole user
skills discovered from `<root>/.config/plugins/<name>/`), `extension` (parse a
`<root>/.config/extensions/<skill>.md` into named `[hook]` sections + catch-all,
and `apply` it to a base `SKILL.md` at `<!-- brain:ext hook -->` markers,
producing a *new workspace copy* only — never the repo/plugin source; unmatched
content lands in a trailing "Personal extensions" section), `render` (base skill
→ installable files, injecting the extension into `SKILL.md`), `layout` (the
workspace `.agents/skills` dir + frontend dirs, and the pure `link_ops` target
computation), `install` (collect bundled + plugins, write workspace skills +
create frontend symlinks; thin FS shell over `link_ops`), `prune` (remove what a
sync no longer produces, plus the `remove_existing` helper), and `command`
(`brain skills sync [--root <sandbox>]`; `format_sync_plan` prints the workspace
skills dir, frontend count, extension/plugin sources, and the prune step before
the FS shell runs; `brain skills status` reports capability selection and
enforcement).

**Pruning is part of every sync.** `install` writes a `.brain-rendered` marker
into each skill directory it renders, so a later run can tell brain's own output
apart from a skill the user wrote by hand in `.agents/skills`. After rendering
the current set, `prune::run` deletes every *marked* directory the sync did not
produce (a deleted or renamed plugin, a bundled skill dropped from the binary)
together with its frontend links, then sweeps frontend symlinks left dangling by
a skill that is gone. It runs **before** user-skill discovery so a leftover can
never be re-adopted as user-authored. Both decisions — `stale_rendered` and
`is_orphan_frontend_link` — are pure; `run` is the FS shell. An unmarked
directory is the user's and is never removed, so skills rendered by a pre-prune
brain are kept until a sync re-renders (and marks) them.
For workspace-only launches, `layout` and `install` also render selected skills
under the workspace UUID and actor cache without creating registry or frontend
links. A machine skill is read only from its exact configured absolute
directory; the source directory, `SKILL.md`, and every descendant must be real
files or directories rather than symlinks. `resync_skills()` (the A seam) runs the pipeline, gated by
`skills_auto_sync` (**default `true`** since the B4 cutover) so a mutation
re-renders workspace skills; set the flag `false` to manage skills only via
explicit `brain skills sync`. The pipeline never writes the user's global agent
registry or global frontend skill directories. See
the B spec under `docs/superpowers/specs/`.

On the first ordinary invocation of a new Brain version, bootstrap also checks
legacy global core-skill locations and renders the embedded core set into every
registered workspace. The pass is version-marked in the machine cache,
continues past individual workspace failures, and does not delete old global
files. TUI startup excludes the selected root because its normal startup sync
handles that root immediately before the brain panel launches.

**Version-stamped auto-resync.** So a version bump ships its *skill* changes the
way it ships *code* changes (immediately, no manual step), `bootstrap` calls
`skills::resync_on_version_change()` for every ready-workspace invocation. It
compares `env!("CARGO_PKG_VERSION")` to a per-workspace render stamp
(`state` DB `meta('skills_synced_version')`) and, when they differ, runs the
same pipeline once, then re-stamps (`needs_resync` is the pure decision). It is
LLM-free, gated by the same `skills_auto_sync` flag, never fails the invocation,
and is a no-op once stamped, so `--help`/`--version` (no workspace), the
internal hook/server, and registry-only maintenance never trigger it, and a
fork with no extensions renders identically. Every authoritative render path
(the version-resync, the mutation `resync_skills`, and a real `brain skills
sync`) writes the stamp so none re-fires redundantly; a `--root` sandbox sync
leaves no stamp.

The bundled `todo` workflow declares `todo:agenda-after-build` as a generic
no-op hook. If an installed extension adds a post-build step, that runtime step
owns every input and output path and passes optional markdown to the generic
helper explicitly. Core does not discover extension artifacts or external
service state.

### `entry.rs`
`Bucket` (Projects / Areas / Resources / Archive; declaration order =
display order, Archive last) and `Entry` (absolute selected-workspace `path`,
home-abbreviated `display`, `bucket`).
`collect()` walks each root with `walkdir`, skips hidden files
(`.`-prefixed) and the root itself, and tags every entry with its bucket.
Missing roots are silently skipped.

### `picker/`
The ratatui fuzzy picker. The `App` type lives in `picker/mod.rs` (so every
submodule reaches its private fields); the impl is split into `haystack`
(match preprocessing), `filter` (constructors + `refilter` + grouping), `nav`
(query edits + cursor + scroll), `selection` (highlighted-entry accessors +
shell-owned palette/confirmation construction data), and `view` (`draw_into`). `App` **owns** its entries (so the persistent
shell can `set_entries` to rescope a bucket in place), precomputed
`HaystackBuf`s, the query, the current matches, and the interleaved
header/match `display_rows`. `refilter()` runs nucleo substring matching,
sorts matches by bucket then score then walk order, and rebuilds the
section-grouped rows. Navigation (`move_up`/`down`, `page_*`,
`ensure_visible`) keeps the cursor and its section header on screen.
Rendering is delegated to `render.rs` and exposed as `draw_into(f, app,
area)` so `tui`'s embedded search panel paints it. It owns no modal state:
`selection` returns the contextual `SearchPalette` or `Confirm` data that the shell
wraps in its one `Overlay` slot. The `App` is driven
key-by-key by the search view (`tui/search_view.rs`): `Enter` opens the
selection in place (a directory falls back to a Finder reveal), `Ctrl-Enter`
reveals, `Ctrl-G` confirms a markdown→PDF conversion, `Ctrl-D` confirms a
trash, and a confirmed palette row runs its action. Every action happens in
place; the shell never tears down on a selection. On `Accept`, Delete trashes
the path and `drop_path`s the entry (`reload_entries` keeps the query), and
the picker stays open.

### `menu/`
Split into `labels` (contextual-row elision), `model`
(`SearchAction`/`Targets`/the row list/`shortcut_for`), and `view`
(`draw_modal`). It has **no screen of its own**: the host opens it with
`Ctrl-p`, drives the shared pure `CommandPalette<SearchAction>` state, and
paints it with `draw_modal` as a centered overlay. `SearchAction` owns only
search-specific operations and wraps `GlobalAction` for application commands.
The row list is built per-open by `items(side, include_msg, targets)` because
the rows are contextual: the **layout
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
The shared state owns the filtered row indices and key handling (each row's
matchable text includes its 1-based number) and returns
`Continue`/`Confirm`/`Cancel`. `Cancel` (Esc) tells the host to drop the
overlay, not to exit. In the persistent shell `GlobalAction::MessageBrain`
opens or focuses the brain panel and `GlobalAction::ToggleLayout` swaps which
side it sits on.

### `confirm.rs`
The shared yes/no confirmation modal. Like `menu`, it has **no screen of its
own**: the shell holds a `Confirm { path, kind, yes }` in its active overlay, the host
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
The app-level main-view axis: the `MainView` enum (`Tasks` / `BrainSearch` /
`Logs`),
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
private Backblaze B2 bucket via `rclone bisync`, dispatched in
`src/command/sync.rs`
**before** the `markdown-to-pdf` prerequisite gate (like `config`/`env`/
`persona`/`skills`). The data flow per run is **lock → refuse an active
rollout → build → prove remote identity → materialize runtime state → run →
post-pass → verify → journal**: `config` (`SyncConfig`, parsed from the brain-env `sync`
block) feeds `remote::build_remote` (the B2 remote as `RCLONE_CONFIG_*` env
vars, never on argv) and `args::bisync_args` (the full `rclone bisync` argv:
conflict resolution bias for the direction, keep-both flags, `--max-delete`,
default excludes (including machine-local dependency trees and Python
bytecode), `--check-access --check-filename RCLONE_TEST`, plus
`--stats 10s --stats-one-line` for live progress and `--resilient --recover`
for resumability). Before any remote or portable mutation,
`identity/` validates the selected root's existing portable manifest, probes
the remote `.config/workspace.json` through rclone, and returns a private
`VerifiedRemote` capability only when its UUID and schema match. The
check-access, bisync, CSV, and counter lanes require that capability. The
UUID-scoped rclone workdir is not created and stale rclone locks are not reaped
until this identity proof succeeds. `identity/claim.rs` owns setup's
append-only `.config/workspace-claims/<uuid>.json` protocol. Concurrent setup
processes publish exact manifest claims under distinct names. A new claim
stages one attempt and returns; retry enumerates and validates the durable set,
elects the lowest UUID deterministically, and re-probes the canonical
manifest, and publish it with immutable-copy defense. A losing claimant
refuses without replacing `.config/workspace.json`;
`check_access.rs` creates/repairs the root-level marker on
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
SQLite journal at `<workspace-cache>/sync/journal.db` (table `sync_runs`,
machine-local, never synced). `command::sync_once` is the thin orchestrator
that runs this whole pipeline; `command::print_status`/`print_conflicts` back
`brain sync status`/`brain sync conflicts`. `command::resolve` backs `brain
sync resolve`: local deletes plus, in `command::resolve_remote`, the remote
half — an `rclone lsf` of the original's own remote directory, matched with
`conflicts::remote_losers_for_original`, then one `rclone deletefile` per
loser. Its rclone shell is an injected runner closure, so the decision logic is
tested without the network. `resolve` runs no bisync and writes no journal
entry. `setup.rs` is `brain sync setup`'s
interactive flow (collect bucket + credentials, validate the local manifest,
probe the remote identity, display the local canonical name/UUID plus configured
target and observed remote status/UUID, and publish and read back the exact
existing local manifest for an empty remote or an explicitly authorized
nonempty manifestless remote). Interactive authorization is an explicit yes;
noninteractive authorization is the exact selected UUID in
`--adopt-workspace-id`. Setup holds the selected workspace's UUID-scoped sync
lock across identity election, safe empty-remote task-schema preparation,
check-access marker bootstrap, and the baseline
`sync_once(Direction::Resync)`. Only `Clean` persists the candidate `sync`
block; attention, abort, and transport failures leave credentials unsaved. The verified
manifest boundary completes before setup writes baseline data.
Mismatched, malformed,
incompatible, and present-but-unreadable remote manifests fail closed. Ordinary
and internal identity gates have no adoption authority and remain nonprompting.
`brain sync repair` reruns just
that resync on an already configured machine, mainly as the recovery path for
rclone's own "prior listings missing" guard. See
[integrations.md](integrations.md) for the rclone handoff detail and
[data-model.md](data-model.md) for the `sync` config fields and the journal
schema. `rclone` is an external dependency (not a Cargo crate): a soft
prerequisite, checked only when `brain sync` actually runs, never a startup
gate (`brain tasks doctor` reports its presence/version informationally).

`WorkspacePaths` is the sole UUID-derived path authority for this pipeline.
Lock, journal, current state/log, rclone workdir, temporary transport files, and
CSV baseline callers receive the selected workspace's paths or an explicit
path derived from them. Sync runtime code never reopens the workspace registry,
resolves a global brain root, or consults HOME for a convenience path.
The gated local transport harness exercises two matching local remotes
concurrently with two production `WorkspacePaths`; it also probes a mismatched
remote manifest before constructing a bisync run. The hermetic rollout
acceptance test composes this path contract with shared-server, actor/task,
merge, triage, and `AgentController` seams without adding a production-only
acceptance branch.

`check.rs` backs `brain check`, a **read-only** sibling of `sync_once`: it
first passes the same remote identity gate, then builds the same
`Direction::Both` argv via `args::bisync_args` and appends
`--dry-run`, runs it through `run::run_rclone_capture` (a quiet, non-streaming
counterpart to `run::run_rclone` — no live terminal output, just `(exit_ok,
combined_output)`), then classifies the captured detection-phase lines with
`progress::classify_change`/`Side` (the same parser `progress.rs` already
exposed for a future live file-list). It then runs the CSV lane's read-only
counterpart in `check/csv.rs`: `check::collect_csv_pending_with_fetch` reads the cached
`csv_sync::baseline_path` text and the local task/habit CSVs, fetches each
remote CSV through `csv_sync::remote_csv_arg` + rclone `copyto`, and compares
both sides to the baseline with UUID-aware, name-aligned
`csv_merge::parse`-backed row diffs. The pure
`check::format_report` receives both the file path lists and the CSV row
counts for the themed summary. The command prints default progress before the
rclone dry-run and before the CSV baseline pass. No journal entry, no conflict post-pass, no
baseline mutation: it never calls `rclone bisync` without `--dry-run`, and
its CSV pass never writes local files, remotes, or baselines.

**The auto-sync trigger layer** (`lock.rs`/`watch.rs`/`periodic.rs`/`trigger.rs`, wired
into the shell lifecycle and receiver dispatch) makes sync automatic while keeping the pure/impure
split. The **pure** cores carry the decisions and the tests; the thin shells do
the IO/threads/`Command`:

- `lock.rs` — one advisory sync lock per workspace UUID at
  `<workspace-cache>/sync/sync.lock` (a PID file beside the journal). Pure
  `is_stale(owner_alive, age, cap)` decides reap-ability (dead owner or
  heartbeat mtime past the cap); `try_acquire(path)` is the atomic
  (`create_new`/O_EXCL) thin IO shell returning `Option<Guard>` (`None` when a
  live, fresh sync holds it), and `Guard` owns a heartbeat thread that refreshes
  the lockfile mtime until drop. Drop stops the heartbeat and removes the file
  only if it still holds our PID. It wraps **all** sync entry points, including
  the manual command path in `src/command/sync.rs`, closing a pre-existing
  concurrent-`brain sync` race.
- `watch.rs` — the pure `Debouncer` (a clock-injected quiescence state machine:
  `on_event`/`time_until_fire`/`poll`) and the pure `is_watch_relevant(path)`
  exclude predicate, plus the thin `notify` shell `spawn_watcher_with` (owns the
  platform watcher, the mpsc event channel, and the debounce loop) and
  `spawn_watcher` (the real auto-sync watcher). `WatcherHandle` owns an explicit
  stop sender and worker join handle. Dropping one TUI's handle stops and joins
  only that workspace's watcher; a peer workspace's watcher keeps running. On
  fire it spawns a detached
  `Direction::Push` run. That direction uses a one-way, non-deleting rclone
  copy; its CSV/counter pass reads remote state only to build a safe upload and
  never writes local state, so the push cannot re-arm its own watcher.
- `periodic.rs`: the live-shell five-minute pull timer. Its owned handle uses
  a stop channel plus a joined worker, so closing one shell stops its timer
  promptly. Each tick launches the same detached pull path as startup, and the
  workspace lock coalesces overlapping timers and manual work.
- `trigger.rs`: the single shell-facing entry point. The pure request builder
  pins the canonical selector and expected UUID; the injected
  `DetachedSyncRunner` boundary makes child launch observable in tests.
  `spawn_detached_sync(workspace, dir)` spawns the current exe as
  `brain --workspace <canonical-name> sync [--pull|--push] --if-idle`, fully
  detached (`process_group(0)` + null stdio), with `BRAIN_WORKSPACE_ID` set to
  the selected UUID. Bootstrap compares that expected UUID with the record
  selected by `--workspace` and refuses a mismatch. Automatic startup,
  periodic, watcher, receiver-freshness, and receiver completion triggers go
  through it, for two reasons: a sync in a
  separate process can never write over the TUI, and a detached child in its own
  process group outlives the shell / terminal close. `--if-idle` makes a
  redundant trigger coalesce (exit silently) rather than follow. There is no
  in-process sync path anymore (the old `run_locked_sync`/`sync_in_background`
  are gone). The parent moves each `Child` into a small waiter thread so a
  completed background sync is reaped and cannot accumulate as a zombie.
- `current.rs` — the in-flight sync's UUID-scoped observable state, so a
  detached background sync stays observable. `Reporter` is the single output sink of a run: each
  line is appended to `<workspace-cache>/sync/current.log` and echoed to the
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
  message needs a downstream pull, plus the shared 250ms status-poll, five-second
  launch-grace, and three-attempt bounds. `journal::latest_downstream_completion`
  deliberately ignores push-only and aborted rows. `tui/app_sync.rs` consumes
  an injected runtime for monotonic/UTC clocks, journal/current-state reads, and
  detached launches, so the finite retry and completion decisions are tested
  without wall-clock sleeps.
- `config.rs` carries `debounce_ms` (default 3000) and
  `debounce() -> Duration`; `command::format_triggers` renders the startup,
  five-minute periodic, change-push, and message-pull policies in
  `brain sync status`.

**The `TuiRuntime` lifecycle seam** separates ordered startup acquisition and
application assembly in `src/tui/runtime/builder.rs` from process-lifetime
execution and teardown ownership in `src/tui/runtime/mod.rs`. The builder's
sync-services stage calls
`trigger::spawn_detached_sync(Pull)` whenever sync is configured and retains a
`periodic::spawn_periodic_puller` handle plus a `watch::spawn_watcher` handle
(when `watch_effective()`). Orderly shutdown drops that TUI's timer and watcher,
which explicitly stop and join only their workers, and performs no exit sync.
`ReceiverRuntime` owns the receiver freshness-gate state and its
observation-driven pure poll transition. `AppServices` owns the injected sync
adapter that reads clocks, journals, and current process state and launches
children, plus the one bounded receiver-attachment coordinator. Attachment
staging runs outside the event-loop thread; each pending tick renews the exact
claim, and a result is usable only after a fresh clock read and exact-owner
renewal. Generation mismatches, lost or expired ownership, and disabled intent
discard the result and clean any local files without advancing the job.
One cancellation authority follows every sequential provider child, including
Resend access refresh and media download. It publishes one process group at a
time, closes the cancel-before-publication race, and kills and reaps that group
on receiver shutdown. Each successful file is atomically renamed from `.part`;
the worker result and prepared run transfer one owning directory guard, so an
unread result or any rollback removes the exact job directory.
The later capability-plan, frontend-probe, resume-validation, registration,
spawn, and tab-allocation boundaries use the same semantic owner gate: each
effect is followed by a new injected-clock observation and exact renewal before
the next effect can begin. Resume validation false/error and exact resume
registration rejection choose portable Fresh recovery only after that renewal;
lost or deferred ownership stops the attempt instead. Failure cleanup completes before the coordinator
samples the observation supplied to the exact-owner retry CAS. Losing or
expiring the claim therefore permits local resource cleanup but no durable job
or retry mutation.
While receiver processing is enabled, the sole receiver tick first scans and
applies every durable `/restart` control. It always continues the one claimed
or active run, including after disable, and claims at most one FIFO job only
when enabled and idle; a claimed job passes the sync-freshness gate, and `/new`
completes before any agent launch. A terminal artifact becomes durable only
when one immediate state transaction proves that the exact session validated
from the artifact is still the locked `completed` lifecycle-native
registration, persists the exact binding, and moves the live owner's launch to
`done`. A mismatch, concurrent
SessionStart rotation, or database error rolls back and leaves the run and
artifact available for the next tick. There is no second process-local cursor,
interactive-panel handoff, activity sample, or socket poll.
`tui/app_sync.rs` passes freshness observations into the runtime and owns the
cross-feature sync launch, task reload, footer, and warning effects at the exact queued-job
consumption boundary. It
queues stale inbound work behind a pull and reloads tasks before dispatch. It
also reloads tasks whenever a new successful downstream journal row appears.
The receiver completion path launches an immediate push before delivery. The
shared server does not own this gate. All paths are gated and best-effort; an
unconfigured brain gets no watcher or automatic sync.

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

### `migration/`

The explicit `brain workspace migrate` coordinator separates pure planning,
privacy-limited backup, durable journal, portable-user mapping, focused step
adapters, and final verification. Legacy, Prepared, Current, and
unsupported-newer states are explicit. The coordinator takes the UUID sync lock
before discovery, planning, or journal creation and retains it through the
complete rollout. Selection, acknowledgement, and the initial remote identity gate finish before journal creation. For a configured
workspace, the journaled final legacy sync runs first; the coordinator then
reloads portable config, users, and both assignment CSVs and reruns mapping
preflight before backup or portable mutation. `migration/mapping_prompt.rs`
holds the pure question text, the numbered member options, and the answer
interpretation, so the `/dev/tty` shell in `migration/users.rs` only reads and
writes lines. An answer that adopts an existing person for an assignment value
becomes an `AssignmentRewrites` entry that the coordinator hands to the
task-schema cutover. The journal lives at
`<workspace-cache>/migrations/multi-workspace-v1.json`; the retained backup
lives under `<workspace-cache>/migration-backups/` and contains only the
portable rollout inventory. The coordinator reuses existing sync, manifest,
users, task-schema, triage, and reindex transactions. Its first journaled step
is the final legacy semantic sync when sync is configured, so UUID task merge
identity never becomes authoritative first. If the remote is already current,
`migration/legacy_join.rs` replaces the ordinary legacy CSV lane with a
replayable, local-only `task_id` bridge. It validates both current remote CSVs,
preserves remote UUIDs on matching display IDs, and leaves local-only UUIDs for
the journaled local cutover. Before the step can complete, it also max-merges
both local counters with the fetched remote counters and floors them beyond the
exact joined task and habit display IDs. The bridge never publishes CSVs or
counters remotely. Task-schema migration and its
remote transition share the UUID sync lock. The transition publishes both
current CSVs, durably establishes both machine-local UUID baselines, and only
then publishes `tasks/SCHEMA.json`; all three paths are excluded from the
generic rclone lane. Every completed step is persisted atomically; rerun
validates the exact workspace UUID/root/plan and resumes the original backup
instead of starting another generation. While the journal exists, ordinary
sync and sync setup refuse at the lock boundary so an interrupted transition
must resume through `brain workspace migrate`. Final verification
checks registry and manifest identity, membership, task UUID and assignment
invariants, triage state, derived indexes, and remote identity before removing
the active journal. Every journaled failure is resume-only because remote
publication can complete before its step record becomes durable. The backup
remains machine-local and retained for forensic or coordinated recovery.

### `project/`
The PARA project record: `model` (the `.METADATA.json` schema, its validators,
and the README template) and the mutations over it (`create`, `set`, `archive`,
`show`). Keyed on an explicit root; unknown metadata fields round-trip through a
serde flatten rather than being dropped. `show` composes the project's task list
with `tasks::scan::chronic` to answer whether every open task has gone quiet.

### `contacts/` and `clean.rs`
`contacts/` is the workspace's local contacts book: `model` (the CSV and id
allocation), `find` (staged resolution and search), `render` (the table). It is
keyed on an explicit root, so each workspace has its own book. `clean.rs` is the
byproduct sweep behind `brain clean` — a closed pattern list, a walk that never
enters `.git` or the agent registries, and a pure report.

### `tasks/`
Everything specific to the **tasks main view**, ported from the old `tasks`
crate under one namespace: `identity` (immutable UUIDs and deterministic
legacy identity), `schema` (a coordinator-only, backup-owning task-schema
migration helper split into canonical columns, path validation, pure transformation, and durable
transaction/recovery modules), `task` (CSV model, legacy-compatible load, and pure
assignment defaults/membership/UI visibility), `view` (sub-views +
`build_view`), `selector` (date parsing), `render` (task-card lines, chrome,
markdown), `shortcuts` (the help/footer catalogue), `complete` (native
task/habit completion), `agenda` (the section-preserving agenda-markdown sync
that every completion runs), `mutate` (remove/defer/touch/backlog/assign, plus
the chunk-family cascade), `backlog` (the review listing and the two silent
maintenance passes), `scan` (the chronic / stale-waiting / tracker-linked
reads), `rules` (the task automation lint and the task ↔ project link),
`habits` (habit defer and completed-occurrence retention), `streak` (named day
streaks), `triage_state` (whether this month's monthly pass has happened),
`triage_habits` (stable managed definitions,
marker-based mutation policy, complete purge, and durable grouped
replacement/recovery split into orchestration, artifact, and journal modules),
`doctor` (health check), `plain` (`--no-tui` printer),
and `cli` (the tasks clap args, nested under `brain tasks`). Reuses the
crate-level `session` / `state` / `pty_pane` shared with the brain-search view.
Native task command runners accept explicit `WorkspaceContext` and
`ActorContext`; they never re-resolve a global root. Shared CSV mutation code
normalizes legacy `assignee` headers to `assigned_to` before any write.
New task and habit rows carry UUIDv4 identity; completion and edits preserve
it, while a spawned habit occurrence gets a new UUID and retains assignment
and `system_key`. The schema helper requires the rollout coordinator to state
that the last legacy semantic sync is complete or not configured. It takes an
existing durable machine-local backup base plus a backup directory beneath
that base; the destination must resolve disjoint from the workspace tree. The
helper creates each missing descendant one component at a time and syncs every
actual parent, including partially created chains found on retry. Every
permanent backup entry and transaction artifact is file-synced and
parent-directory-synced before live replacement. A durable
prepared/committed journal makes failure or interruption between replacements
recoverable; retry rolls a prepared generation back before migrating, or
finishes committed cleanup. Only explicit `brain workspace migrate` invokes
it. Legacy CSVs keep `task_id` as their first sync key until that coordinated
migration runs. After migration, `sync/csv_merge/` owns
name-aligned UUID merge (`table` + `merge`), deterministic mutable display-ID
allocation (`reconcile`), and dependency/project reverse-link rewriting
(`relationships`). `csv_sync/operation.rs` fetches and validates the remote
schema marker, then preflights the matching local manifest and every
base, local, and remote task/habit table as one operation before any write;
`csv_sync/metadata.rs` stages project metadata and republishes every
authoritative metadata file so retries heal partial remote publication.
`csv_sync/transport.rs` is the rclone-facing adapter that batches staging,
downloads, and publication; the parent retains merge policy and setup
classification.
Its publication result distinguishes local filesystem failures from remote
transport failures so command diagnostics identify the failing boundary.
`counters` consumes display-ID floors from the reconciled tables only after
that operation succeeds. Ordinary sync never activates the migration helper.

`agenda/` follows the crate's pure/impure split. The decision is pure and
filesystem-free: `doc` splits an agenda into a preamble plus `## ` sections and
reassembles it byte-for-byte, `lines` edits one section body (drop-by-id,
renumber, chunk swap), `derive` rebuilds the CSV-derived snapshot sections, and
`sync` composes them into one `sync_markdown(text, id, action, snapshot, today)`.
`io` is the thin shell: it resolves the day's targets from `agenda_markdown_dir`,
`agenda_dir`, and `markdown_to_pdf_path`, reads and writes the file, and
regenerates the printable only when one already exists. Resolution is keyed on
`(RegistryStore, WorkspaceContext)` rather than a `CommandContext`, so the HTTP
habits route — a real mutation surface holding a verified workspace and its
registry, but no command context — can sync like every other path. Every failure there is
logged and swallowed — the CSVs are committed before the sync runs, so the
agenda can never fail a completion. `complete::complete_and_sync_agenda` is the
one native entry point (CLI and tasks view alike);
`complete::complete_in_workspace_without_agenda_sync` exists only for callers
that own agenda handling themselves. `brain tasks sync-agenda` exposes the same
code to external mutators, so the bundled `/todo` scripts shell out to it
instead of carrying a second implementation.

App construction reconciles managed triage state before loading its final task
and habit vectors. Task reindex does the same before generic Python rules and
retention cleanup. Explicit sync repair reconciles only after a clean repair
outcome; ordinary sync does not gain a second policy branch.

### `tui/` (the merged shell)
The persistent shell, built from the ported tasks `tui/` and extended with the
main-view axis. `App` is an eight-field composition root:

```text
TuiRuntime (process lifetime)
└── App (cross-feature mediator)
    ├── AppContext       immutable command, workspace, config, frontend, and path identity
    ├── TasksState       task materialization, filtering, selection, and logical day
    ├── BrainPanelState  controllers, tabs, actors, interactive-session state,
    │                    and skill/receiver sessions
    ├── ShellState       main view, focus, layout, search, logs, and selected tab
    ├── Overlay          the one active cross-feature modal
    ├── AppServices      runners, state DB, session store, receiver intent refresh,
    │                    sync effects, and bounded attachment staging
    ├── StatusState      triage gate, live toggle, messages, and sync status
    └── ReceiverRuntime  enabled intent, freshness gate, durable-run handle,
                         and BR-18 legacy endpoint lifetime
```

The context is replaced as a complete immutable snapshot when portable config
is refreshed. It never owns the mutable triage or task materialization days:
`StatusState` and `TasksState` own those values respectively. `AppServices`
exposes semantic effects instead of one getter per injected object, and
`BrainPanelState` keeps every frontend behind `AgentController`. Installing a
main controller derives the completion actor from that controller, so the live
controller and its response-validation identity cannot diverge. Skill-session
tab identities advance monotonically with checked exhaustion; a rejected tab
does not mutate the tab collection or counter and its launched controller is
shut down. Receiver-tab insertion additionally rejects and shuts down a second
simultaneous receiver controller before it can mutate tab state.

`Overlay` remains a top-level field because it mediates mutually exclusive
modals spanning tasks, search, status, and the brain panel. `ReceiverRuntime`
also remains top-level because it holds receiver intent, freshness, and the one
durable run across ticks. Its narrowly named legacy job socket only preserves
builder/lifetime compatibility until BR-18 and has no consumer behavior. The injected receiver intent refresher is a
cross-feature server-control effect owned behind `AppServices`; App invokes its
semantic receiver-action operation without obtaining the adapter. Cross-feature
launch, database, receiver lifecycle, task refresh, and focus changes remain App
operations; neither focused owner receives the whole App.
Immutable context and brain queries are consumed through their focused owners
instead of being mirrored as App accessors. App methods remain only where an
operation actually coordinates multiple owners.

`TasksState`
owns task/habit source rows, view materialization, assignment and query
filtering, selection, notes expansion, rendered body lines, and viewport
layout. `ShellState` owns the active main view, panel focus and side, the brain
panel's hit-test rectangle, the embedded brain-directory `picker::App`, the
logs view, and the selected brain-tab identity. Both expose semantic
transitions and purpose-specific results rather than their internal field
representation. Task link selection, managed-task removal validation, and
daily-triage matching stay inside `TasksState`; they return owned effect plans
or decisions for `App` to execute. The task renderer consumes a narrow,
iterator-backed panel model whose storage remains private to the aggregate.
Advancing the task aggregate's logical
day rematerializes its date-relative view immediately from its owned rows, so
a later CSV reload failure cannot leave the new date paired with an old body.
Task fuzzy matching lives under `state/tasks/filter.rs`. Immutable
workspace/runtime identity lives in `AppContext`; agent and skill-session
controller state lives in `BrainPanelState`; injected runners, the state DB,
and sync effects live in `AppServices`; and transient status lives in
`StatusState`. Cross-feature coordination remains on `App`. `App` also owns
exactly one `overlay: Option<Overlay>`. The data-bearing variants cover the task palette,
brain input, task confirmation, search palette, search confirmation, link
picker, assignee filter, help, and sync log. This makes simultaneous modals
unrepresentable; `overlay/mod.rs` owns the pure open, replace, route, and close
transitions. Input routing and drawing exhaustively match that same enum, so no
boolean precedence model can disagree with what is visible. The task
aggregate's assignment context carries the effective
actor, portable-member rows, and the one-person/shared visibility decision.
`TasksState` also owns the process-scoped assignee filter; its captive member
picker remains overlay state. Startup validates `--assigned-to` against the
assignment context and seeds the task state; view materialization retains the
complete base set so
body rebuilding can switch members or clear to all before fuzzy matching. Plain
output instead applies assignment as a final render filter. Header composition
renders assignment only from the focused task panel model, while non-assignment CLI
filters remain in the static chip row. The context's
detail mode controls task-card rendering, and its create, reassign, and filter
flags independently gate their palette rows. A missing portable registry uses
a one-actor compatibility context with hidden assignment controls.
`event_loop` routes keys in the precedence documented in
[keybindings.md](keybindings.md): app-level accelerators (view switch, help,
panel focus/scroll, brain open/close/new, quit) → captive modal → brain panel
(forward bytes) → active main view (`handlers` for tasks, `search_view` for the
brain-directory picker). `draw` renders the active main view in the main
panel and the brain panel beside it (`ShellState::panel_side`). The task
renderer accepts `&mut TasksState` plus a small cross-feature chrome context;
the log handler accepts `&mut ShellState`, and task-search handling accepts
`&mut TasksState`. Shell search input mutates the embedded picker locally and
returns a closed `SearchEffect` for file, overlay, or refresh work that `App`
must coordinate. The brain renderer accepts a `BrainPanelContext` assembled by
the top-level mediator from display values and the active controller; it never
receives `App`. The logs renderer accepts only the selected `LogsView`. Task and app-level overlays
span that composed shell; search palette and confirmation variants stay
centered inside the search `main_area`, matching the picker's pre-shell render.
`search_view.rs` is the brain-directory view's handler (its picker nav, in-place
open, and the search-specific variants of the shell overlay). The remaining
submodules (`handlers`, `keymap`, `palette`, `modals`, `links`, `draw_*`,
`app_*`, `shell`) are the tasks view's. The assignee picker has its own
`draw_assignee` module so the shared-workspace overlay stays separate from the
general confirm, link, and brain-input modal renderer.

The larger submodules are directories split by concern: `runtime/`
(`mod.rs` for the process owner, `tick.rs` for recurring coordination,
`shutdown.rs` for lifecycle order, and `terminal.rs` for the RAII terminal
owner), `action/` (the closed
`GlobalAction` enum), `handlers/`
(`overlay`/`tasks_view`/`input`), `event_loop/` (`setup`/`modal_route`/`run`),
`draw/` (`tasks_panel`/`brain_panel`/`layout`, with the `draw` entry in
`draw/mod.rs`), `state/`
(`context`/`tasks`/`brain`/`shell`/`services`/`status`), `palette/`
(`model`/`command`/`state`), `app_state/` (`construct`/`view`), `app_actions/`
(`commands`/`receiver`/`triage`, with pure triage policy in
`triage/decision.rs`), `app_brain/` (`launch`/`lifecycle` plus receiver
`dispatch`/`active`/`artifact`/`reply` coordinators and focused tests), and
`tests/` (split by
area). `BrainPanelState` owns the main persistent controller, while
`app_brain/` coordinates isolated receiver dispatch and terminal delivery;
`app_brain/launch/session.rs` owns the full fresh-or-resume launch transaction,
while `launch.rs` keeps capability construction, transport selection, and the
public app actions.
`BrainPanelState` delegates its one shared ephemeral-tab collection to
`state/brain/ephemeral.rs`. Skill sessions and receiver runs retain distinct
metadata variants, but both use one lifetime-monotonic `SessionTabId` allocator,
controller store, title order, and shutdown pass. Checked allocation exhaustion
shuts down the rejected controller before returning and leaves both the
collection and counter unchanged. `app_brain_tab.rs` owns shared observation and
keyboard navigation; `app_skill_session/` retains skill completion signals and
configured-skill lifecycle. `BrainPanelState` exposes background-only receiver
insertion, observation, controller access, and removal. It rejects and shuts
down a second simultaneous receiver controller before insertion, so one
workspace process cannot own more than one live receiver run.

Receiver-only tabs do not make a hidden brain panel visible. When the panel is
already visible, skill and receiver titles render and navigate in their shared
stable insertion order. Receiver insertion and terminal removal preserve the
current main view, effective tab, panel visibility, and keyboard focus. The
`Overlay` owner and transitions live in
`overlay/mod.rs`. The per-variant state
structs (`TaskPalette`, `ConfirmState`, `BrainInputState`, `HelpState`,
`SyncLogState`, `LinkPickerState`, `AssigneeFilterState`, and the confirm enums) live in `modal_state.rs` with
`pub(super)` fields; shared panel and tab types live in
`model.rs`, while `mod.rs` keeps only the coordinating eight-field `App` type,
narrow shell entry exports, and module wiring. Receiver representation is
private to `receiver/runtime.rs` and its focused sync child.
`receiver/planning.rs` owns the frontend-neutral durable job plus
conversation to `SessionPlan` and initial-prompt decision without owning tab,
claim, or coordinator state. The runtime receives sync observations and never
reads journals, files, or process state or launches sync children. Cross-feature
work that touches the brain aggregate, task reloads, sync processes, response
files, or provider delivery remains in the existing App coordinators.
`status_warning.rs` validates receiver
phone configuration and renders persistent warning content independently from
the transient palette flash.

`tui/mod.rs` is the composition boundary, not a shared name hub. It declares
the module tree, owns `App`, and exposes only the narrow shell entry types used
by the command layer. Child modules import production dependencies from their
declaring owner modules with explicit names. They do not inherit sibling names
through `use super::*`, import the TUI root as a wildcard, or rely on wildcard
child re-exports. This keeps a dependency change visible at the consumer and
prevents an unrelated owner from becoming an accidental public surface.

### Startup and shutdown (`TuiRuntime`)

`run_tui()` only starts `TuiRuntime`, runs it, and requests orderly shutdown.
The runtime's named builder stages acquire the workspace UUID singleton,
refresh hooks and skills, bind the UUID-scoped `jobs.sock`, complete a bounded
connect/elect/register handshake with the machine-wide server, and start its
heartbeat worker. The
handshake retries only stale or missing generations, while authoritative
workspace rejection ends startup. It next resolves assignment state, acquires
the terminal, opens the state DB, builds the brain-search picker
(`build_search`), and assembles
one internal `AppInit` request. `App::new(AppInit)` initializes the
lifetime-free shell model from that owned request.
Before opening the state DB, `TerminalSession::acquire` opens `/dev/tty`, builds
the ratatui terminal, and records raw mode, alternate screen, mouse capture,
mouse-motion adjustment, optional keyboard enhancement, and cursor-restoration
ownership in one guard. A later startup or event-loop error therefore restores
the terminal through the same path as an orderly return. Acquisition failure
rolls back every mode that may already have been enabled. Explicit restoration
is idempotent. An optional keyboard-pop failure is logged and retained for a
best-effort retry without replacing an event-loop or required-cleanup result;
`Drop` retries every remaining capability and logs required cleanup failure
without panicking.
The constructor derives its retained root and state-DB path from that context;
callers cannot supply competing workspace paths. `open_or_focus_brain(None)`
then launches the selected frontend through an `AgentController`. Claude
validates transcripts, OpenCode validates exact-root live sessions, and Codex
validates its exact on-disk rollout; each resumes or starts fresh from that
adapter-owned evidence. `focus_tasks()`
returns focus to the tasks main view so `j`/`k` work at once. The sync-services
stage then wires a detached pull-biased startup sync and retains the optional
watcher and periodic puller. The runtime owns the `App`, `TerminalSession`,
workspace singleton, heartbeat worker, watcher, periodic puller, shell instance
identity, and the App state that holds the session lock and the legacy receiver
endpoint lifetime.
From successful server registration through final runtime assembly, one partial-
startup owner retains the heartbeat lease before the bound job socket. Assignment
resolution, terminal acquisition, DB/config/model construction, initial-panel
launch, startup workers, and the lifecycle-completeness check all run inside that
boundary. Any fallible return therefore unwinds its newer resources, unregisters
and drops the server lease, and only then removes the job socket. The socket moves
into the App only during the final infallible runtime assembly.

One runtime tick coordinates the established order: close an exited agent panel
and refresh tasks if needed, drain heartbeat/server-health events, tick skill
sessions, tick the receiver, poll sync status and conditionally refresh tasks,
then poll the triage gate and conditionally refresh tasks. Manual refresh has a
second explicit order: advance the logical day, reload tasks, check triage only
after a day rollover, then report the refresh. The terminal loop itself contains
only runtime tick, draw, terminal poll/read, and one application update call.

Orderly shutdown is idempotent. It stops the heartbeat worker and attempts the
bounded unregister before shutting down the main and all ephemeral controllers,
drops the periodic puller and watcher, releases the shell's session lock, then
restores the terminal. The singleton remains held until the runtime itself is
dropped, after every owned resource has completed its orderly teardown. `Drop`
reuses the same sequence as a best-effort fallback and logs restoration errors
without panicking. Agent-controller and session-lock teardown failures are also
logged instead of discarded. The final accepted unregister stops the shared process. No
exit sync exists. The **daily-triage nudge**
is coupled to that startup sync: when a configured startup sync is pending, the
runtime does *not* run the check immediately. It captures the sync journal's latest
clean downstream row ID, kicks the sync, and calls `App::arm_triage_gate`
(deferral, no modal). The recurring tick coordinator then calls
`App::tick_triage_gate`.
Once a newer clean pull/both/resync row appears, it strictly reloads portable
config, reconciles managed policy under the workspace task-store owner, reloads
both synced CSVs, and evaluates the live process-scoped alert state. Palette
re-enable while this gate is armed defers its check to that refreshed state;
the gate does not cache the launch-time alert value. If another overlay owns
the exclusive slot after refresh, the gate retains only the refreshed alert
decision and retries delivery after that overlay closes; it does not reload or
reconcile the sync twice. Reload or
reconciliation errors are logged and shown in the TUI rather than discarded,
so the modal reflects post-sync completion state (pure `triage_gate_resolved`
decides resolution). `enable_daily_triage_check=false` disables only the final alert;
the same gate still performs the strict config, managed-policy, and task-table
refresh. With no startup sync, the check runs immediately as before. The
brain
panel is **closeable** (agent exit → `close_brain` shuts down its controller and the main
view goes full-width); `open_or_focus_brain` (`Ctrl+M`) re-opens it. The
brain-directory view keeps its own `scope`/`rescope`/`search_refresh` for
bucket rescoping (`Ctrl+R` / palette search rows). Unlike the pre-merge shell
there is no `Exit` enum — the shell just returns from the event loop on quit
(the tasks view never handed a plan back), and `Ctrl+T`/`Ctrl+B` switch views
in-process rather than exiting.

### `pty_pane.rs`
`PtyPane` is a dormant-capable `AgentTransport` that spawns a complete
`LaunchSpec` under a pseudoterminal (`portable-pty`),
streams its bytes through a `vt100` parser, and exposes the screen for
rendering. Reader / writer / waiter threads; `send` / `resize` /
`scroll_*` / `is_alive`. It applies the spec's selected workspace cwd before
the child starts and has no frontend-specific command or input knowledge. It
clears the inherited process environment, then applies only the launch spec's
selected workspace/actor values, hook metadata, narrow frontend necessities,
and a fixed `TERM`. The command string runs through fixed `/bin/sh -c`, which
preserves configured command parsing without sourcing login or interactive
profiles that could recreate filtered environment values.
Real-PTY transport regressions live in the owned `pty_pane/tests.rs` child so
the production module remains focused on transport behavior.

### `session.rs`
Compatibility launch planning: re-exported `agent::AgentKind`,
`Plan::{Resume,Fresh}` (chosen from actor-scoped DB resume candidates), and
`build_llm_command`, which adds the legacy shell `cd` prefix around the command
translated by the selected adapter and returns a typed error for a blank legacy
session ID. `env_for` and `env_for_skill_session` remain only
as compatibility helpers for pure callers and tests. Live TUI panels build
complete `LaunchRequest` values: the adapter supplies common workspace identity
and `BRAIN_AGENT_KIND`; the main panel's `HookMetadata` adds instance, PID,
state DB, and response attribution, while a skill-session panel adds only
`BRAIN_SESSION_DONE_URL` and `BRAIN_SESSION_TOKEN`. `claude_cmd`, `codex_cmd`,
`opencode_cmd`, and `default_agent_frontend` (which frontend this machine opens
with no selector flag; resolved in `agent::default_frontend` right after
bootstrap, since env needs a workspace) are machine-local brain env values. The three functional
configured commands are spliced in
verbatim so they may carry their own flags, and brain never depends on a shell
alias.

### `workspace/selector.rs`
The selector every suggested command echoes back, plus strict-selector
enforcement. Bootstrap records the resolved canonical name once; message builders
read it through `suggest("sync setup")` so a remediation reads
`brain sync setup -w family` rather than sending someone to the default
workspace. The same module owns `BRAIN_REQUIRE_WORKSPACE`: Brain sets it on the
children it spawns, and `bootstrap` refuses such a child that names no workspace,
so a code path that forgets `-w` fails instead of silently targeting the default.
Both decisions are pure (`with_selector`, `violates_strict_selector`).

### `skill_session/`
The skill-session model and its cross-process completion bridge — one dedicated
ephemeral session per prompt, in its own brain-panel tab (see
[features.md](features.md) and [integrations.md](integrations.md)).

- `mod.rs` — the pure model: `SkillSessionKey` (`DailyTriage` or `Custom(index)`),
  `SkillSessionSpec` (`title` / `prompt` / `command_label`), `available` (builtin
  daily triage, gated on the workspace's daily-triage check, plus the parsed
  `skill_sessions` env array), and `runnable` (what may be *started* now: offered
  minus running, the decision that hides a row while its session runs).
- `prompt.rs` — the launch prompt: the workspace's prompt plus the appended
  completion protocol, and the `BRAIN_SESSION_DONE_URL` / `BRAIN_SESSION_TOKEN`
  names. Pure, so a user's own skill needs no brain-specific edits.
- `signal.rs` — the on-disk bridge. Pure `parse_signal` (which also rejects a
  token that isn't safe as a file name, since it arrives in a request body) +
  `ready_to_close` (the close gate: every path the run declared in `require` must
  exist; core declares none, so an empty list closes at once), plus a thin file
  shell (`record_done` / `read_signal` / `clear` / `clear_all`, one file per token
  under `<workspace-cache>/skill-sessions/`): the brain server writes it from
  `POST /local/<lease>/w/<ingress>/session/done`, and the matching TUI polls each
  open tab's own token each tick, holding a premature signal until its required
  outputs land. Deliberately ignorant of *what* those outputs are — see the
  extension-agnostic rule in [AGENTS.md](../AGENTS.md). The route resolves a live
  lease and verified workspace context before selecting the signal path, so one
  workspace cannot close another workspace's tab.
- `editor.rs` — the `brain env set skill_sessions` walkthrough (pure list
  arithmetic + a thin prompt shell).

### `state/`
The SQLite state layer (`rusqlite`, WAL) at `<workspace-cache>/state.db`.
`brain_sessions` tracks Claude, Codex, and OpenCode sessions by a composite agent-kind,
session-ID, workspace-UUID, actor-ID, and channel key with a `locked_pid` lock
and `active`/`completed` completion status;
`meta` stores the `panel_side` layout preference and the
`skills_synced_version` render stamp (the brain version that last rendered this
workspace's skills, read by `skills::resync_on_version_change`). The resume
model is scoped lock + recency behind `agent::session::SessionStore`
(`reap_dead_locks`, `sessions_by_recency`, `claim`, `register`, `release`,
`mark_active`, `mark_completed`, `completion_status`). The `PanelSide` enum lives here since
it's the persisted value. `receiver/` separately owns durable logical
conversations, immutable inbound jobs, explicit lifecycle and retry state,
expiring owner fences, transcript/native-session bindings, and schema v10
recovery metadata. Schema v7 added the retry origin required to distinguish
pre-acceptance launch retries from progressed recovery work; schema v8 added
exact receiver-session registrations; schema v9 added opaque job tokens,
post-spawn lifecycle timestamps, and the bounded observation cursor; schema v10
added independent lifecycle deadlines, attempt identity, the lifetime/current
observation split, the pending unavailable-notice intent, and the exact
superseded instance/session cleanup fence.
See
[data-model.md](data-model.md) and [integrations.md](integrations.md).

### `server/`
Brain has one machine-wide, TUI-lifetime shared process. It serves the local
habits and triage routes, owns the public route grammar, and authenticates and
durably admits receiver requests only for enabled workspaces with live TUI
leases.

The lifecycle is closed around those TUIs except for the explicit browser-only
habits lease. Startup binds the workspace-local
job socket before election and registration; heartbeats renew only the
registered lease; recovery re-enters the election after a stale generation.
The final orderly unregister stops the process immediately, while the watchdog
stops it when the final crashed lease reaches TTL. A background habits lease
keeps the process alive without a TUI until `brain habits kill`; a TUI
registration replaces that lease. If a peer TUI keeps the
process alive but the selected target is unavailable, the handler sends one
unavailable response and creates no job. Accepted work is stored in the exact
workspace DB, but the shared process has no offline routing mode, queue
consumer, or agent launcher.
- `server/router.rs` — pure exact-component mapping for the two machine-wide
  provider paths `/sms` and `/email`, which name no workspace, and for
  capability-protected local
  `/local/<lease>/w/<ingress>/{habits,habits/done,session/done}` paths. Global,
  malformed, missing, and extra-component routes are rejected, including the
  retired ingress-scoped provider paths.
- `server/receiver/routing.rs` — pure selection of one workspace from the
  destination a provider payload names (Twilio's `To`, a Resend payload's
  `to`/`cc`) matched against each registered workspace's own
  `twilio_from_number` / `resend_from_email`. Returns the workspace, `Ambiguous`
  when two publish one address, or `Unknown`; never a guess.
- `server/workspace_route.rs` — resolves the typed ingress through the live
  lease table first; a provider route reaches it through the ingress this
  process remembers for the addressed workspace. Shared-process routing captures a generation-bound lease
  ticket under the control-state mutex, reloads and verifies the registry,
  root, and portable manifest without that mutex, then revalidates the exact
  live authority revision before returning a `WorkspaceContext`. Heartbeats
  preserve that revision; registration and enablement changes create a new
  authority incarnation. Removal or expiry leaves no accepting authority, and
  any later registration advances the remembered revision even when every
  lease field is reused. Revision advancement is checked before the lease
  transition, so an unrepresentable next revision leaves all authority state
  unchanged. `workspace_route/loader.rs` owns the injectable verified-context
  loader, leaving the parent focused on route authority and error semantics.
- `server/request.rs`: the HTTP facade after pure routing. It resolves local
  capabilities or provider destinations, applies body limits, dispatches the
  selected route, and translates receiver failures to provider-facing
  responses. `server/mod.rs` remains the thin lifecycle and URL entry point.
- `server/http/` — the shared process's bounded, connection-closing HTTP/1.x
  request parser and response writer. Request heads are capped at 16 KiB, IO
  starts with one absolute two-second monotonic parse deadline, and each
  accepted connection carries one request. Local actions retain that deadline
  through response flush. Receiver requests keep it through the bounded body
  and local provider verification, then enter one fixed 30-second
  provider/handoff/response phase, but cannot enter it after the parse
  deadline has elapsed. The parser rejects conflicting or repeated
  framing, unsupported transfer codings, invalid field names, and malformed
  or over-limit chunk/trailer grammar. Field values strip only HTTP
  `SP`/`HTAB` optional whitespace; forbidden controls and Unicode whitespace
  are rejected.
  Chunk extensions are outside the deliberately extension-free safe subset.
- `server/http_workers.rs` — a fixed four-worker, process-lifetime HTTP set
  over a loopback `std::net::TcpListener`. A start gate prevents any worker
  from accepting until all four spawns succeed; partial startup therefore
  rolls back before a body can be consumed. Workers route before reading any
  local action body, and local habits and triage bodies are capped at 16 KiB.
  The lifecycle/control loop never owns body IO or waits to join a held worker
  during final-TUI shutdown.
- `server/receiver/` owns the ordered inbound pipeline. `http/` loads only the
  selected workspace's provider configuration after ingress resolution;
  `http/sms.rs` and `http/email/` return typed provider outcomes while they
  verify and normalize provider input; Resend retrieval is capped at 1 MiB per
  response and ten seconds per request; `http/email/body.rs` is the pure
  prompt-shaping half: HTML-only mail becomes readable text and the result is
  bounded at 16 KiB with an explicit truncation notice before it enters an
  isolated receiver launch prompt;
  `dispatch.rs` resolves the selected workspace's portable actor, while
  `dispatch/deliveries.rs` owns in-flight provider-ID exclusion and the bounded
  verified-unavailable Email discard set. Durable provider deduplication and
  queued capacity live in `state::receiver`; `dispatch/final_authority.rs` owns
  the repeated persisted-intent and exact-TTL admission checks, and
  `dispatch/pipeline.rs` owns the durable acceptance call. Verified unavailable
  Resend IDs enter discard memory before any state DB is opened, so later
  availability cannot replay them into a TUI. An exact in-flight ID records a
  deferred discard and receives 503 until the pending acceptance resolves;
  `admission.rs` linearizes cancellable exact-lease admission with revocation,
  while `dispatch/tests/late_revocation.rs` exercises the production pipeline's
  synchronized final admission boundary and `dispatch/tests/deliveries.rs`
  covers provider-ID state; the receiver-specific DB open installs the short
  absolute handoff remainder before configuration and migration, then dispatch
  refreshes and rebinds that remainder before acceptance lock waiting;
  `job.rs` defines
  the immutable serialized `InboundJob`; `control.rs` is the pure reading of a
  message as a `/new` or `/restart` command plus the queue cut a restart makes;
  `unavailable.rs` owns the one-response,
  no-retry discard result; and `attachments.rs` stages media for the TUI.
- `server/control/` owns the bounded newline-delimited JSON protocol. `codec.rs`
  caps frames, requires one frame followed by EOF, and applies one absolute
  deadline before every read, write, and flush attempt, including successful
  progress. `connect.rs` creates a safe nonblocking Unix socket and polls it
  only until that same deadline, without spawning an unjoinable connector.
  `client.rs` carries the deadline through connect, write, and read, and
  performs a bounded connect/elect/register handshake for startup and recovery.
  `status.rs` owns non-electing process and exact-workspace inspection. Receiver
  status reads the process record once, then obtains live lease count and exact
  receiver state from one generation-bound response. Both status requests use
  immutable lease projections, so they never prune TTLs or advance lifecycle
  state. Ordinary register, heartbeat, enablement, unregister, ingress lookup,
  and routing-availability transitions filter expiry without removing it. The
  focused `server/` children keep the state machine separated by responsibility:
  `listener.rs` owns socket IO, `registration.rs` owns live-TUI filesystem
  validation, `shared_request.rs` owns two-phase deadline-bounded requests, and
  `receiver_authority.rs` owns route/admission transitions plus the only
  expiry-removal path and exact watchdog revocation. Ordinary table paths
  filter expiry without consuming it. Their tests are split into request/deadline, route-authority,
  receiver-admission, and shared-fixture modules. The watchdog supplies periodic pruning and guarantees final crashed-lease
  shutdown without traffic. The generation-bound workspace-ingress lookup
  returns only the ingress from that workspace's exact live accepted registration.
  `server.rs` copies validation capabilities under the state mutex, reopens registry plus
  manifest identity, compares the TUI-resolved root without retaining it,
  derives the UUID-local job socket, and verifies the live singleton and
  listener through the request's bounded connector without that mutex, then
  rechecks generation and deadline before creating a lease. An
  exact replay of an already-accepted registration is idempotent, while any
  competing lease or changed identity remains rejected. `heartbeat.rs` renews or generation-safely
  re-elects and re-registers after a crash through an injected scheduling seam.
- `server/security.rs` owns pure Twilio HMAC, Resend/Svix HMAC, and the ordered
  authenticate-then-resolve decision for enabled portable identities.
- `server/lifecycle/` owns the shared-process boundary. `paths.rs` places
  `process.json`, `control.sock`, `election.lock`, and `server.log` below one
  machine-wide directory. `state.rs` owns the minimal generation-tagged record;
  `election.rs` owns the pure start decision, directory advisory mutex, exact
  owner checks, and parent-to-child token handoff with retained parent cleanup
  until child publication. Its explicit, bounded completion retries transient
  mutex contention only while the exact parent token remains. Fallible token
  inspection reports filesystem and JSON failures without consuming the
  cleanup capability, so callers may repair and retry;
  `process.rs` owns detached election orchestration, retained elected-child
  observation through `Child::try_wait`, immediate lifetime-waiter ownership
  for each published elected child before parent handoff cleanup, the hidden
  server loop, and signal
  cleanup; `watchdog.rs`
  applies clock-injected expiry plus the bounded initial-registration deadline;
  `lease.rs`, `table.rs`, and `decision.rs` own typed leases and latched
  final-shutdown decisions; `table/status.rs` owns immutable status
  projections, while `table/transition.rs` owns pure revision/identity helpers
  and table tests are split by registration versus expiry behavior. Signal
  flags and cleanup ownership precede process
  state publication. The table and process record never contain roots, users,
  credentials, prompts, logs, or message bodies.
  `lifecycle::pid_alive` remains the stable seam for sync callers.
- Receiver intent mutations live at the registry boundary. One pure
  `receiver_transition` decision feeds CLI start/stop, startup
  `--with-receiver`, and palette toggles. `RegistryStore` reloads under its
  interprocess transaction and verifies the selected canonical name still owns
  the expected UUID before saving. After persistence, a generation-bound
  control refresh names only the workspace UUID; the shared process reloads
  the authoritative record and updates a matching live lease if present. This
  path never elects a process. The UUID-local job socket is only a live-endpoint
  validation marker; the TUI does not read or dispatch jobs from it. Receiver command
  dispatch and setup remain in `command/server/receiver/mod.rs`; the exact
  mutation, refresh-warning, and status decisions live in its focused
  `enablement.rs` child, with their tests under `enablement/tests.rs`.
- `server/routes/habits/` — the habits MVC route and embedded frontend. GET
  and completion POST handlers receive an already-resolved workspace context;
  the rendered page preserves that context's opaque ingress and exact live
  lease capability in its POST URL. A lease that supersedes a browser-only
  background lease for the same workspace inherits that one capability for its
  own lifetime, so a page rendered before a TUI started keeps routing.
  Unknown, no-live-TUI, unavailable-root, and identity-mismatched routes
  are rejected and never fall back to the machine default.
- `server/routes/session/` - the capability-protected local skill-session
  completion controller: an ephemeral skill session's workspace-scoped completion
  signal (see `skill_session/signal.rs`).

The shared HTTP process resolves receiver ingress availability before loading
credentials, users, prompt data, or the workspace state DB. A request is
accepted only after provider authentication, actor resolution, final authority
revalidation, and an atomic insert or deduplication in the exact workspace's
durable receiver queue. Complete serialized jobs remain bounded at 1 MiB. The
acceptance transaction resolves provider and exact-job duplicates before it
counts `queued` rows, then rejects a sixty-fifth queued job without inserting a
conversation. Progressed, retrying, failed, and done rows do not consume that
queued capacity. Failed-storage, full, disabled, and missing targets receive
one channel-specific unavailable response and create no new row. Successful
provider IDs are authoritative in SQLite, not process memory; memory only excludes
simultaneous requests and retains up to 1024 verified-unavailable Email
discards. A verified unavailable duplicate of an in-flight ID receives 503
until the pending admission resolves and promotes its deferred discard.
Immediately before durable admission, dispatch reserves the final five seconds
for the HTTP response and derives one handoff deadline capped at two seconds
and at the start of that response reserve. It revalidates the retained
generation, authority revision, receiver enablement, and live lease under the
control mutex. The commit path reloads persisted intent outside the mutex, then
acquires control once, samples the monotonic clock, revalidates the exact
route and admission identity, and atomically commits the cancellable admission
before unlocking and entering durable enqueue. Disable, unregister, and disable-enable
ABA either cancel before commit or wait only until the control request's
absolute deadline outside the mutex. A deadline rejection performs no later
disable or unregister mutation. Watchdog expiry removes the exact lease and
cancels every matching pre-commit admission before shutdown. Exact route and
TTL authority are checked again immediately before admission commit, without
waiting for the watchdog interval. The same final admission boundary first reloads the selected
canonical registry record and requires the exact workspace UUID's persistent
receiver intent to remain enabled, so a lost live-refresh notification cannot
let a raced disable enqueue. Only after that filesystem IO does the combined
commit operation acquire control, sample the monotonic clock inside the lock,
revalidate exact live authority, and perform the admission CAS before unlock.
It then installs the exact remaining duration before SQLite WAL configuration
or schema reconciliation. After the database opens, dispatch recomputes and
rebinds the remainder before acceptance lock waiting. The deadline is checked
again after commit before provider success. Provider and database IO never run
while the control mutex is held.

Queued inbound work never interrupts the interactive main panel or another
receiver run. The one recurring receiver tick claims new work only while
persisted intent is enabled and its local run is idle. Disabling intent does not
abandon a pending or active claim: the same tick continues renewal, freshness,
completion, child-exit, and cleanup management until that run is terminal. An
already-claimed no-attachment run may finish after its freshness boundary;
pending attachment IO is cancelled without blocking, while the exact claim is
retained and renewed until staging can restart after re-enable. The tick
renews an already claimed job before honoring the sync-freshness gate, then
claims the oldest ready durable row by `(received_at_unix_ms, job_id)`, plans
through `AgentController`, and launches a new background receiver tab. One
background staging generation holds that FIFO position, so later arrivals
remain durable and unclaimed until the active run reaches a terminal
outcome. Receiver insertion,
completion, and removal preserve the active main view, tab, panel visibility,
and keyboard focus. A second receiver-run insertion is rejected and its supplied
controller is shut down without changing tab state or focus.
The idle claim transaction holds SQLite's immediate writer reservation while
it checks for a queued exact `/restart`. A ready restart refuses the ordinary
claim so the next control scan can apply its cut. Ingress that commits after
the claim instead sees an already active owner, which the restart deliberately
preserves. An already claimed `/new` may finish after intent is disabled, but
its fresh boundary returns to idle and cannot claim following work until intent
is enabled again.
`tui/app_sync.rs` holds inbound dispatch behind a pull when downstream state is
more than two hours old and exposes current sync state to the footer and
palette. The common receiver prompt classifies task-capture requests as task
creation instead of immediate execution, unless the sender explicitly asks for
both, and ends with the trusted job token's exact marker. Claude and Codex
submit and post-tool hooks feed the same content-free Python observation
writer. OpenCode derives acceptance from incremental user-message parts and
progress from its post-tool callback using bounded correlation, with no history
scan on either path. Root `session.updated` events conservatively seed resumed
native correlation while parent-bearing child updates remain ineligible. Native
Claude/Codex `agent_id` child hooks are rejected. The owner-only,
advisory-locked JSON snapshot is at most
4096 bytes; same-directory flush and atomic replacement make its monotonic
revision safe across duplicate or concurrent delivery. It retains the first
accepted, progressing, and completed timestamps plus the latest monotonic
progress timestamp, but never stores prompt, tool, response,
sender, recipient, attachment, cwd, credential, or transcript content.
Before deriving a later phase, the writer descriptor-walks the absolute cache
path without following links, makes the opened workspace-cache and observation
directories owner-only, and performs lock, temporary-file, replacement, and
directory-sync operations relative to the confined directory descriptor. A
symlinked ancestor or lock leaf fails closed. It opens any prior snapshot in
nonblocking mode where supported and validates the same
regular owner-only descriptor, byte bound, exact schema, lifecycle, and
token/instance/session lineage before and after one bounded read. A replaced,
malformed, or ambiguous entry is preserved and cannot be normalized into
trusted evidence by a later hook.
Each new boundary time is clamped to the latest retained producer evidence, and
a later pulse must strictly advance that producer time, so a wall-clock rollback
cannot publish an invalid timeline. The constructed
snapshot is validated again before replacement.
Revisions stop at SQLite's maximum signed integer. Once saturated, later
producer events preserve the last valid snapshot instead of writing evidence
the durable store cannot represent. The bridge represents a trusted exact Stop
over saturated accepted or progressing evidence as terminal settlement without
a snapshot mutation, distinct from rejection. This lets the stop transaction
publish its completed-session row and separate artifact while the durable store
retains the maximum cursor.
Completion may create revision 1 with null intermediate timestamps when submit
and tool evidence was missed; private text remains separate. A
verified artifact completion retains the strict lifecycle completion timestamp
when the same poll observed it, even when that producer evidence is later than
the renewed lease. After claim renewal and exact artifact/lifecycle validation,
the App samples a separate fresh clock for terminal authorization. The producer
timestamp never participates in the lease comparison; when no completed
lifecycle boundary exists, that fresh App time is also the durable fallback.
The committed completion then launches an immediate push. A synchronous PTY
spawn failure releases the exact registration and claim, shuts down the new
controller once, and records a durable pre-spawn retry with a clock observation
sampled after cleanup, without changing the main panel. Successful spawn is the
no-auto-replay boundary: Brain immediately reauthorizes and commits `launched`
before tab allocation. Owner loss or a store error before that commit preserves
the exact registration and leaves `launching` fenced; allocation or later owner
loss preserves `launched`. None of those successful-spawn outcomes becomes a
retry. Exit, shutdown, and lease expiry clean local resources but do not release
durable correlation or make any fenced state claimable again; BR-16 owns that
ambiguous recovery policy. Every
channel's final body is shaped by `server/reply/`, whose `plain_text/`
submodules (`block.rs` for line-level scaffolding, `inline.rs` for spans) are a
pure markdown-to-plain-text pass applied to SMS before the length decision;
email keeps its markdown. Provider replies are handed
to the bounded background worker in `server/delivery.rs`, keeping network
latency off the TUI event loop. Receiver bodies are capped at 1 MiB by the
shared parser, and the shared fixed worker set prevents one slow provider call
from blocking every route. The final orderly lease stops the process
immediately; final crashed-lease cleanup follows the lifecycle TTL.
Orderly shell teardown runs the receiver-specific stage before generic agent
controller shutdown. It cancels, reaps, and joins attachment work and may record
an exact Planning retry only for work that has not spawned. For a successful
spawn it removes the exact local receiver tab, artifact, observation, and lock
without releasing or retrying the durable correlation. Expired `launching`,
`launched`, `accepted`, and `processing` rows stop FIFO unchanged until BR-16
wires the schema-v10 policy into its recurring recovery transaction.

### `lib.rs`
Re-exports the modules so integration tests in `tests/` and the thin binary
entry point share one compiled module graph. `main.rs` calls this library
surface instead of privately declaring every module a second time.

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
- **The binary's stdout is only intentional short-lived CLI output:**
  `config/env/version`, `workspace list`, the `receiver` details listing and the
  `receiver email` / `receiver phone` addresses, explicit plain-task output,
  help, and non-TUI logs mirrored by `--verbose`. Clap errors and diagnostics go to
  stderr. The TUI renders to `/dev/tty`.
- **Every `SearchAction` has exactly one applicable palette row** (guarded by
  tests on `items(side, …)`) so the search catalog cannot silently drop an
  action. Shared task/search rows also assert the same `GlobalAction` while
  preserving each surface's contextual label and direct-key metadata. Every
  direct shortcut for a `GlobalAction`, including Close brain, Show tasks,
  Message brain, and Open agenda, enters `App::execute_global_action`; closing
  an active skill-session tab remains a skill-session operation.
- **The brain panel is open at startup but closeable.** `tui` launches the
  selected controller at startup and is two-panel; when its agent
  exits the panel **closes** (search goes full-width) — it does not quit the
  shell. `open_or_focus_brain` ("Message brain" / `Ctrl-M`) re-opens it.
- **Exactly one frontend session per brain instance is locked at a time.**
  A session-start bridge may update an exact registered tuple or rotate an
  already registered active lineage; it rejects unregistered events and frees
  the instance's other sessions on every accepted start
  (so `/new` leaves the prior conversation resumable). Its authorization and
  mutation share one `BEGIN IMMEDIATE` transaction, so concurrent rotations
  recheck target ownership after serialization. `release` clears the lock on
  exit; dead-PID locks are reaped on the next startup.
- **A committed completion always has a durable response artifact.** The Stop
  hook stages a unique synced file, acquires `BEGIN IMMEDIATE`, rechecks and
  updates the same exact locked session scope, publishes and syncs the artifact,
  then commits. A failed publication or commit rolls back and cleans up only
  that attempt; a concurrent SessionStart rotation is rechecked after writer
  serialization.

## Dependencies

Beyond the picker's core (clap, ratatui, crossterm, nucleo, walkdir,
anyhow), the persistent shell pulls in four crates, all mirroring the `tasks`
sibling so the two projects share a stack:

- `portable-pty` + `vt100` + `tui-term` — spawn, parse, and render the
  selected agent frontend's embedded PTY.
- `rusqlite` (`bundled`) — the WAL state DB shared with the SessionStart
  hook; `bundled` avoids a system libsqlite dependency.
- `uuid` (`v4`, `v5`): fresh runtime/workspace/task identities plus
  deterministic task identities for fixture-tested legacy migration.
- `include_dir` — embeds the repo's `skills/` dir (SKILL.md + scripts) into the
  binary so a public cloner needs no repo checkout; `brain skills sync` writes
  them out. Multi-file skill assets rule out `include_str!`. It embeds the tree
  exactly as it sits on the *building* machine, so `embed::is_build_artifact`
  drops build litter (`__pycache__/`, `*.pyc`/`*.pyo`, `.DS_Store`) on the way
  in: a `.pyc` records the absolute path it was compiled from, which would ship
  the builder's filesystem layout inside a public binary.
- `signal-hook`: installs safe SIGINT/SIGTERM flags for the shared process.
  The accept loop observes the flag and lets its generation owner remove only
  the matching process record and control socket, without unsafe signal code.
- `fs2`: provides Rust-1.85-compatible advisory locking for the shared-server
  election mutex, avoiding unsafe platform calls while serializing exact owner
  reaping and parent-to-child adoption.
- `nix` (`fs`, `poll`, `socket`): provides safe nonblocking Unix-socket setup,
  readiness polling, and socket-error inspection for the shared control plane.
  Stable `std` has no cancellable Unix-domain `connect`, so this small wrapper
  enforces the total deadline without unsafe code or detached helper threads.
- `notify` (8.x) — cross-platform filesystem observation for the **C4
  auto-sync watcher** (`src/sync/watch.rs`). Linux uses the recommended native
  backend. macOS uses notify's one-second `PollWatcher`, because FSEvents can
  silently omit valid changes; this was reproduced by the real-filesystem
  watcher integration test. All decision logic remains in the pure,
  clock-injected `watch::Debouncer`, so we depend on neither
  `notify-debouncer-full` nor `notify-debouncer-mini`.
- `pulldown-cmark` (`default-features = false`, `html`) — CommonMark parsing for
  the email channel's HTML part (`src/server/reply/html.rs`). The agent answers
  in markdown and a mail client renders HTML, so the two have to be bridged;
  a correct CommonMark implementation is not something to hand-roll for an
  email body. Default features are off to drop the crate's bundled CLI and
  `getopts`. The *outbound SMS* direction stays on brain's own line-oriented
  `reply/plain_text/` pass, which is intentionally not a parser — see
  `docs/decisions.md`.

Architecture tests depend directly on **`syn`** with its `full` and `visit`
features. The active TUI dependency guard uses a complete Rust syntax tree to
pin event-loop routing without adding a parser to the shipped binary. The
former receiver-queue guard's direct `proc-macro2` dependency is gone with that
obsolete in-memory consumer boundary.

`brain sync` also depends on **`rclone`**, but as an external command it
shells out to (`src/sync/run.rs`), not a Cargo crate: brain builds the argv
and an env-var-only remote config and lets the user's own `rclone` install do
the transfer. It's a soft prerequisite (checked only when `brain sync` runs;
see [integrations.md](integrations.md)), unlike the hard `markdown-to-pdf`
gate.
