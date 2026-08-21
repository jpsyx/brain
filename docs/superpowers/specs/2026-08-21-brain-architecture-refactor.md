# Brain TUI Architecture Refactor

- **Date:** 2026-08-21
- **Status:** Design complete, ready for implementation planning
- **Review basis:**
  [2026-08-21-architecture-review.md](../2026-08-21-architecture-review.md)
- **Scope:** Maintainability, clarity, ownership, and change cost in the
  persistent TUI. User-visible behavior and correctness changes are out of
  scope.

## 1. Purpose

Brain's command entry point, workspace contexts, agent facade, server receiver
pipeline, and sync seams are already credible. The persistent TUI is the
exception. It is physically split across many files, but those files share one
flat namespace and mutate one 96-field `App`. The result is easy to extend
locally and hard to reason about globally.

This refactor turns the TUI into an application shell with explicit ownership:

```text
tasks command
  -> TuiLaunch
  -> TuiRuntime
       -> TerminalSession
       -> App
            -> AppContext
            -> TasksState
            -> ShellState
            -> BrainPanelState
            -> ReceiverRuntime
            -> Overlay
            -> AppServices
            -> StatusState
       -> RuntimeServices
            -> singleton lease
            -> shared-server lease
            -> sync watcher
            -> periodic puller
            -> shell session lock
```

The refactor does not replace `App`. It changes `App` from a shared field bag
into a mediator over feature-owned state.

## 2. Architectural laws

The implementation must preserve these laws throughout the migration:

1. **No user-visible behavior changes.** Existing keybindings, palette labels,
   main-view behavior, agent launch behavior, receiver semantics, sync
   triggers, task filtering, and terminal presentation remain unchanged.
2. **No correctness redesign.** Existing tests characterize behavior. A test
   may be strengthened to express an architectural contract, but the refactor
   does not use architecture work to change product semantics.
3. **One owner per invariant.** Mutable state that must change together lives
   behind one type. Callers request semantic operations instead of editing its
   fields.
4. **Pure decisions, thin effects.** Routing, queue admission, lifecycle
   planning, visibility, and transition decisions remain pure where practical.
   Terminal, filesystem, process, database, sync, and provider delivery remain
   thin impure shells.
5. **Explicit dependency direction.** Production TUI modules import the types
   and functions they consume. The TUI root must not re-export every child into
   one wildcard namespace.
6. **The application mediator coordinates, features decide.** Cross-feature
   work stays on `App` or a named coordinator. Feature-local decisions and
   mutations live with the feature state.
7. **The existing agent boundary stays intact.** Every frontend continues to
   flow through `AgentController`. `AgentFrontend`, the registry, and the three
   adapters are not redesigned.
8. **The existing receiver policy stays intact.** Incoming jobs remain bounded
   to 64, in memory, and owned by the live workspace TUI. There is no durable
   or headless queue in this project.
9. **No new dependency.** The standard library and existing crates are
   sufficient.
10. **Every production change follows red, green, refactor TDD.** Structural
    moves are protected by characterization or architecture tests before the
    move.

## 3. Scope and non-goals

### In scope

- Behavior-named test modules and included sections in place of numbered
  fragments.
- An owned `TaskViewOptions` runtime model.
- An owned `TuiLaunch` request and one-argument `run_tui` entry point.
- An internal `AppInit` request so model construction has one boundary.
- A single data-bearing `Overlay` enum.
- Search overlays owned by the shell rather than by `picker::App`.
- A shared `GlobalAction`, one global action executor, and reusable palette
  row/state types.
- `InboundQueue` as the only owner of queue representation and capacity.
- `ReceiverRuntime` as the only owner of receiver-local state.
- Explicit receiver tick decisions and effects at the application boundary.
- `TerminalSession` as the RAII owner of terminal modes and restoration.
- `TuiRuntime` as the process-lifetime composition root.
- One named recurring tick coordinator and one application event-update
  boundary.
- Focused task, shell, brain-panel, status, service, and context state
  aggregates under `App`.
- Explicit TUI imports and removal or narrowing of the Clippy allowances that
  exist for the current flat namespace.
- Architecture, integration, decision, testing, and glossary documentation
  needed to describe the final module shape.

### Out of scope

- A durable, offline, cross-process, or headless receiver queue.
- Receiver correctness or delivery-policy changes.
- A fourth agent frontend or changes to frontend-specific behavior.
- Splitting `AgentTransport` into process and viewport traits.
- A generic plugin framework for main views, commands, overlays, or palettes.
- Sync transaction changes.
- A typed command `InvocationPlan`.
- Bounded control-listener workers.
- Narrowing the crate's public module surface.
- New keybindings, new commands, or new configuration variables.
- Performance optimization except the representation-neutral move from
  `Vec::remove(0)` to `VecDeque::pop_front()` inside `InboundQueue`.

## 4. Target boundaries

### 4.1 `TaskViewOptions`

The TUI must not retain a parsed clap object. Add an owned runtime model near
the task view code:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskViewOptions {
    pub filters: Filters,
    pub sort: String,
    pub reverse: bool,
    pub full_notes: bool,
}
```

The exact field organization may mirror existing `Filters` and `DisplayOpts`
when those types can be cloned without weakening their API. The contract is
what matters:

- it owns all data;
- it contains only values used after command dispatch;
- it has a pure conversion from `&tasks::cli::Cli`;
- task view building and header rendering accept the runtime model;
- `App` has no lifetime parameter and stores no `Cli`.

CLI parsing remains in `command::tasks`. One-shot task commands continue to use
their clap types at the boundary.

### 4.2 `TuiLaunch` and `AppInit`

`command::tasks` assembles one owned request:

```rust
pub struct TuiLaunch {
    pub command_context: CommandContext,
    pub view: ViewSpec,
    pub task_options: TaskViewOptions,
    pub agent_kind: AgentKind,
    pub today: NaiveDate,
    pub csv_path: PathBuf,
    pub tasks: Vec<Task>,
    pub habits: Vec<Task>,
    pub active_view: Option<View>,
    pub initial_search: Option<String>,
    pub skip_daily_triage_check: bool,
}
```

Fields may be private with a constructor. `run_tui` accepts only `TuiLaunch`.
The unused `with_receiver` argument disappears.

Startup resolves assignment, config, DB, panel side, search entries, server
identity, and injected production services, then passes one private `AppInit`
to `App::new`. `App::new` initializes a model. Startup side effects stay in the
runtime builder instead of migrating into `AppInit`.

### 4.3 Application state composition

The final `App` should have a small, stable top level. Exact names may adapt to
the code, but ownership must match this shape:

```rust
pub(crate) struct App {
    context: AppContext,
    tasks: TasksState,
    shell: ShellState,
    brain: BrainPanelState,
    receiver: ReceiverRuntime,
    overlay: Option<Overlay>,
    services: AppServices,
    status: StatusState,
}
```

Responsibilities:

| Aggregate | Owns |
| --- | --- |
| `AppContext` | immutable command/workspace/actor identity, config, selected frontend, paths, logical-day context |
| `TasksState` | task and habit collections, view selector, filter/search model, selection, line layout, scrolling, assignment filter, note expansion |
| `ShellState` | main view, panel focus and side, search picker, logs view, panel rectangles, active tab selection |
| `BrainPanelState` | main controller, turn state, skill-session tabs, session identities, actor/session metadata, test transports |
| `ReceiverRuntime` | queue, job socket, intent, lease, remote-turn identity, timing, retry, probe, and sync-gate state |
| `Overlay` | the only active modal and all modal-local state |
| `AppServices` | injected runners, DB/session store handle, receiver intent refresher, receiver sync runtime |
| `StatusState` | transient flash, persistent warning, alert, sync status, next status poll |

These are ownership boundaries, not a demand for getters around every field.
Code in the aggregate's own module can use its private fields directly. Other
features use semantic methods. `App` may inspect multiple aggregates when it is
coordinating a cross-feature action.

### 4.4 Overlay contract

Replace independent modal options with one enum:

```rust
pub(crate) enum Overlay {
    Palette(CommandPalette<AppAction>),
    BrainInput(BrainInputState),
    TaskConfirm(TaskConfirmState),
    SearchConfirm(confirm::Confirm),
    LinkPicker(LinkPickerState),
    AssigneeFilter(AssigneeFilterState),
    Help(HelpState),
    SyncLog(SyncLogState),
}
```

The action type and palette state may be non-generic if that produces a clearer
Rust API. The required properties are:

- `App` has one `Option<Overlay>`;
- `picker::App` owns only picker data and no shell overlay;
- opening any overlay replaces the prior overlay intentionally;
- key routing and drawing match the same enum;
- closing takes or clears the active variant;
- no boolean precedence structure such as `ActiveModals` remains.

Task confirmation and search confirmation may keep distinct state types because
their interaction models differ. They are unified by ownership, not forced
into one artificial data schema.

### 4.5 Action and palette contract

Application-level actions have one identity:

```rust
pub(crate) enum GlobalAction {
    MessageBrain,
    CloseBrain,
    ToggleReceiver,
    ToggleLayout,
    OpenTasks,
    ShowReceiverStatus,
    ShowReceiverLogs,
    ShowBrainLogs,
    ReturnToTasks,
    OpenHabits,
    SyncNow,
    ShowSyncStatus,
    OpenAgenda,
    ToggleDailyTriageAlert,
    ShowMainBrainSession,
    RunSkillSession(SkillSessionKey),
    ShowSkillSession(SkillSessionKey),
}
```

The final variants should reflect the existing catalog exactly. Search actions
and task actions remain feature owned:

```rust
pub(crate) enum AppAction {
    Global(GlobalAction),
    Search(SearchAction),
    Task(TaskAction),
}
```

One `App::execute_global_action` handles every `GlobalAction`, regardless of
which main view supplied the row. Feature executors handle their feature
actions.

Both palettes use one reusable state and row contract:

```rust
pub(crate) struct PaletteRow<A> {
    label: String,
    action: A,
    shortcut: Option<&'static str>,
}

pub(crate) struct CommandPalette<A> {
    rows: Vec<PaletteRow<A>>,
    filter: String,
    selected: usize,
    context: PaletteContext,
}
```

Generic or erased action storage is an implementation choice. Do not introduce
trait objects merely to unify two enums. Catalog builders remain pure and add
global rows plus contextual feature rows in a deterministic order. Labels and
shortcut hints remain unchanged.

### 4.6 `InboundQueue`

Only `InboundQueue` may know that the queue uses `VecDeque` or has capacity 64:

```rust
pub(crate) struct InboundQueue {
    jobs: VecDeque<InboundJob>,
}
```

Its semantic surface covers:

- `len`, `is_empty`, and `front`;
- staged admission with an opaque token;
- rollback of the exact staged tail when final socket acknowledgement fails;
- commit or finalization of staged admission;
- pop or commit of the head after dispatch;
- cancellation or draining operations used by existing receiver controls;
- a test-only iterator or snapshot when assertions need visibility.

No caller receives `&mut VecDeque` or performs index arithmetic. The admission
token must prevent a rollback from removing a different job. Capacity remains
64 and the socket protocol continues to enqueue before sending its final
accepted acknowledgement.

### 4.7 `ReceiverRuntime`

`ReceiverRuntime` owns all receiver-local mutable state, including the queue and
job socket. Its fields are private outside its module. The rest of the TUI asks
semantic questions and sends events:

```rust
pub(crate) enum ReceiverEvent {
    Tick(Instant),
    JobAdmitted,
    TurnStarted,
    ActivityObserved(ActivitySample),
    CompletionObserved(Completion),
    DeliveryFinished,
    RetryDue,
    RestartRequested(Channel),
    NewSessionRequested(Channel),
}

pub(crate) enum ReceiverEffect {
    PollSocket,
    RefreshIntent,
    RefreshSyncFreshness,
    DispatchHead,
    SamplePanel,
    DeliverCompletion,
    ShowDelayNotice,
}
```

The exact event and effect variants should be derived from existing behavior,
not invented ahead of need. The contract is:

- pure functions choose which stage is due and update receiver-local state;
- the application mediator executes effects requiring `AgentController`, sync,
  provider delivery, files, or panel replacement;
- effect results return as receiver events or focused semantic method calls;
- the terminal event loop does not inspect receiver fields;
- receiver modules do not gain direct ownership of unrelated task or shell
  state.

This is not a mandate for one giant finite-state enum. Orthogonal timers and
identities may remain separate fields when one enum would multiply states.

### 4.8 `TerminalSession`

`TerminalSession` owns the terminal and every mode enabled during acquisition:

- raw mode;
- alternate screen;
- mouse capture;
- disabled mouse motion reporting;
- keyboard enhancement mode when supported;
- cursor visibility;
- `Terminal<CrosstermBackend<File>>`.

It provides `acquire`, `terminal_mut`, and idempotent `restore`. `Drop` performs
best-effort restoration if orderly shutdown did not. Acquisition must roll back
already-enabled modes if a later acquisition step fails.

The restoration sequence remains deterministic and testable through a pure
cleanup plan or a focused injected operation seam. Tests must not require an
interactive `/dev/tty`.

### 4.9 `TuiRuntime`

`TuiRuntime` is the process-lifetime composition root:

```rust
pub(crate) struct TuiRuntime {
    terminal: TerminalSession,
    app: App,
    singleton: singleton::Guard,
    server_lease: Option<HeartbeatWorker>,
    watcher: Option<WatcherHandle>,
    periodic_puller: Option<PeriodicPullHandle>,
    instance: String,
    shutdown: bool,
}
```

It exposes:

- `start(TuiLaunch) -> Result<Self>` for ordered acquisition;
- `run(&mut self) -> Result<()>` for the terminal loop;
- `tick(&mut self, now)` for recurring work;
- `shutdown(&mut self)` for deterministic, idempotent teardown.

`Drop` calls best-effort shutdown. Existing RAII owners remain RAII owners; the
runtime composes them rather than duplicating their internals.

Shutdown preserves the current semantic order unless characterization proves a
more constrained order:

1. unregister or stop the shared-server lease;
2. shut down agent controllers;
3. stop periodic and watcher services;
4. release the shell's session lock;
5. restore the terminal;
6. release the singleton when the runtime drops.

The event loop becomes a small sequence:

```text
tick recurring services
draw current app state
poll terminal input
update application from one event
repeat or exit
```

One named tick coordinator owns the recurring order for receiver, skill
sessions, triage, sync status, heartbeat checks, and refresh work. The event
loop calls that coordinator instead of listing every subsystem.

### 4.10 Import and module boundary

The final `tui/mod.rs` is a composition and export boundary. It may declare
modules and re-export a small intentional entry surface such as `run_tui` and
`TuiLaunch`. It must not glob-re-export child implementations.

Production TUI modules use explicit imports from their owning modules. Glob
imports may remain in a tightly scoped test module when they materially improve
fixtures, but any Clippy allowance must be local to that module. Remove the
global `wildcard_imports` and `redundant_pub_crate` allowances if no unrelated
production use needs them. If an unrelated use remains, narrow the exception
and document it rather than preserving the current TUI-wide excuse.

## 5. Migration sequence

The sequence is intentionally incremental:

1. Add an architecture guard against numbered test fragments, observe it fail,
   then rename and regroup the fragments by behavior.
2. Introduce runtime task options and launch DTOs; remove the `App` lifetime.
3. Establish the single overlay invariant.
4. Unify application actions and palette mechanics.
5. Encapsulate the queue representation.
6. Move receiver-local fields behind `ReceiverRuntime`.
7. Extract receiver tick decisions and application effects.
8. Add terminal RAII.
9. Add the runtime owner and simplify the event loop.
10. Move task and shell state into their aggregates.
11. Move brain-panel, service, context, and status state into their aggregates.
12. remove the wildcard namespace mesh, finish documentation, and run all
    structural and quality gates.

Every step leaves the branch green and reviewable. Temporary compatibility
methods are allowed between steps when they are private, clearly named, and
removed by the task that consumes them. Do not leave forwarding accessors whose
only purpose is to preserve the original flat field bag.

## 6. Test strategy

### Architecture tests

Add or extend directory-wide architecture checks for:

- no `part_<number>.rs` test fragments;
- no `App<'a>` or stored `&Cli` in production TUI code;
- no independent shell overlay options after the overlay task;
- no raw `Vec<InboundJob>` receiver queue after the queue task;
- no receiver field access outside the receiver boundary after encapsulation;
- no TUI-root wildcard re-export or production `use crate::tui::*` after the
  import task;
- agent concrete frontend types remain private to `src/agent`.

Prefer directory-wide checks over exact-file substring checks so semantic file
moves do not create false failures.

### Pure unit tests

- `TaskViewOptions` conversion and owned behavior.
- Overlay routing and close/replace behavior.
- Palette filtering, selection, row ordering, labels, shortcuts, and shared
  global identity across views.
- Queue capacity, staged admission, exact rollback, head commit, control
  removal, and ordering.
- Receiver transition and tick planning for each existing lifecycle phase.
- Terminal acquisition/cleanup plan and idempotent restoration.
- Runtime shutdown planning and event update decisions.

### Characterization and integration tests

- Existing TUI key, draw, agent, skill-session, receiver, sync, and task tests
  stay green after each move.
- Socket acknowledgement and rollback tests use `InboundQueue` through the
  public internal contract.
- Existing all-frontend tests continue to cover Claude, Codex, and OpenCode.
- No live agent, network, or interactive terminal is used in tests.

### Required gates

After focused red/green cycles, each task runs the relevant release tests. The
final task runs:

```sh
rust-loc
cargo test --release
cargo clippy --release --all-targets -- -D warnings
```

It also runs the repository's privacy and documentation checks and verifies
that `git diff --check` is clean.

## 7. Documentation contract

Update durable docs in the same task that changes a boundary:

- `docs/architecture.md`: module map, TUI startup, application composition,
  event flow, and queue ownership.
- `docs/integrations.md`: receiver ingress to queue to controller flow and
  runtime lifecycle ownership.
- `docs/decisions.md`: single overlay, shared global action, live queue owner,
  runtime RAII, and why agent/sync boundaries were preserved.
- `docs/testing.md`: architecture guards and headless lifecycle tests.
- `docs/glossary.md`: only if a plain-English term changes or a new user-facing
  term is introduced. Internal type names alone do not require glossary terms.

No features or keybindings change, so `docs/features.md` and
`docs/keybindings.md` should change only if implementation discovers an
existing description that is now inaccurate.

## 8. Acceptance criteria

The refactor is complete when all of the following are true:

1. `App` has no lifetime and stores no clap `Cli`.
2. `run_tui` accepts one owned `TuiLaunch`.
3. `App::new` accepts one internal initialization request and performs no new
   startup side effects.
4. `App` composes focused state aggregates rather than exposing a flat field
   bag.
5. Exactly one overlay can be represented.
6. Search picker state owns no palette or confirmation overlay.
7. Global application actions have one enum identity and one executor.
8. Both command palettes use one row/state abstraction.
9. Only `InboundQueue` owns queue representation, ordering, and capacity.
10. Only `ReceiverRuntime` owns receiver-local mutable state.
11. Receiver ticks produce explicit decisions or effects executed by the
    application mediator.
12. `TerminalSession` restores terminal state on orderly return, error, and
    drop.
13. `TuiRuntime` owns process-lifetime resources and deterministic shutdown.
14. The terminal loop delegates recurring work to one named tick coordinator.
15. Production TUI code has no root wildcard import or wildcard re-export
    mesh.
16. No numbered test fragments remain.
17. Agent frontend behavior, receiver policy, task behavior, sync behavior,
    keybindings, and terminal presentation remain unchanged.
18. No new dependency is added.
19. `rust-loc`, release tests, release Clippy with warnings denied, privacy
    checks, docs checks, and diff checks are green.

## 9. Deferred follow-ups

The architecture review records four credible follow-ups that are not required
for this implementation:

- `InvocationPlan` for bootstrap and dispatch classification;
- bounded `ControlWorkers` for shared-server control connections;
- a narrow intentional public library surface;
- a separate terminal viewport capability if a second transport needs it.

They should be considered only after this refactor has reduced the TUI's
coupling and made their actual change cost visible.
