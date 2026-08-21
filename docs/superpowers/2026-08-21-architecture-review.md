# Brain Architecture Review

Date: 2026-08-21
Status: Findings and architectural decisions complete
Scope: Maintainability, clarity, ownership, and change cost. Correctness is out
of scope except where an invalid state or unclear lifetime makes maintenance
harder.

## Executive conclusion

Brain has several strong architectural foundations. The binary entry point is
thin, workspace and actor context are resolved once, agent frontends sit behind
a real facade and adapter contract, and the shared receiver server has an
explicit ordered admission pipeline. These should be preserved.

The main maintainability problem is the persistent TUI. Its directory is split
into many files, but the runtime is still one flat module namespace around a
96-field `App`. Twenty files add inherent `App` implementations, 17 wildcard
re-exports flatten TUI children into the module root, and 37 production TUI
files depend on that flattened namespace. This is physical modularity without
enforced ownership.

The receiver queue exposes the consequence most clearly. Socket admission,
queue mutation, control messages, dispatch, completion, delivery, retries,
session leases, activity probes, and sync freshness all mutate fields directly
on `App`. There is no queue owner or receiver runtime aggregate. The queue's
64-item, live-TUI-only policy is sound and should remain; its internal ownership
is what must change.

Phase 3 should therefore focus on one coherent outcome: turn the TUI into an
application shell with explicit runtime, model, overlay, command, and receiver
boundaries. Lower-value changes to command routing, sync orchestration, the
library export surface, and the agent transport viewport can wait until that
boundary exists.

## Review method and baseline

The review used the following evidence:

- `rust-loc` after Phase 1: 780 Rust files, 130,495 total lines, and no file over
  the repository's 400-line review threshold.
- Direct source and module-declaration inspection.
- Internal top-level dependency scans from `crate::<module>` references.
- Focused call tracing for agent launch, TUI startup and teardown, event ticks,
  receiver socket admission, queue dispatch, completion, and server routing.
- Existing architecture, integration, decision, and testing documentation.
- Existing architecture-characterization tests and their sensitivity to the
  Phase 1 module moves.

Quantitative signals after Phase 1:

| Signal | Count | Interpretation |
| --- | ---: | --- |
| Fields on `tui::App` | 96 | Too many independent responsibilities for one model |
| Production files with `impl App` | 20 | Behavior ownership is distributed across the TUI tree |
| Wildcard re-exports in `tui/mod.rs` | 17 | TUI children form a flat shared namespace |
| Production TUI files using the flattened namespace | 37 | Dependency direction is implicit |
| Arguments to `run_tui` | 12 | Startup has no request object or runtime facade |
| Arguments to `App::new` | 22 | Construction is a service locator and startup script |
| Public top-level library modules | 33 | The test/binary seam exposes most implementation modules |
| Numbered `part_XX.rs` test fragments | 89 | LOC compliance without semantic discoverability |

The top-level internal dependency scan also showed `tui` with the largest
fan-out at 25 modules, followed by `command` at 20. High fan-out is expected at
a composition root. It is a concern in `tui` because the same root also exposes
and mutates every feature's state.

## Current architecture map

```text
main
  -> workspace bootstrap
  -> command dispatch
       -> short-lived command families
       -> tasks launch
            -> run_tui
                 -> terminal and process setup
                 -> App (tasks, search, logs, overlays, agents, receiver, sync)
                 -> event loop
                      -> recurring service ticks
                      -> render
                      -> modal, global, view, and panel key routing

shared HTTP process
  -> pure route classifier
  -> workspace route ticket
  -> DispatchPipeline
  -> UUID-local TUI job socket
  -> App.receiver_queue
  -> App.tick_receiver
  -> AgentController
```

The upper command path and the server-to-controller path are understandable.
The architectural compression happens after `run_tui`, where lifecycle,
state, decisions, and side effects converge on `App`.

## High-priority findings

### AR-1: The TUI module tree does not enforce feature ownership

Evidence:

- [`tui/mod.rs`](../../src/tui/mod.rs) explicitly says `App` lives at the root
  so every submodule can reach its fields.
- The same file re-exports children with `pub(crate) use ...::*` so sibling
  modules can call each other through `use crate::tui::*` or `use super::*`.
- `Cargo.toml` disables `wildcard_imports` and `redundant_pub_crate` globally and
  names this TUI pattern as the reason.
- State is grouped only by field ordering. Receiver state alone occupies more
  than 30 adjacent fields, while tasks, panels, overlays, runtime services, and
  persistence share the same struct.

Impact:

- A file move can silently broaden or break dependencies because imports do not
  describe what a module consumes.
- Any `impl App` can mutate any feature's invariants.
- Adding a feature increases the central state bag and the number of implicit
  sibling dependencies.
- File-size splits reduce navigation length but not coupling.

Decision:

Refactor `App` into a composition root over focused state aggregates. Initial
aggregates will cover tasks, brain panels and sessions, overlays, receiver, and
shell navigation. Feature methods should live with their aggregate when they do
not require cross-feature coordination. Cross-feature actions remain on a small
application mediator. Replace TUI-root wildcard imports with explicit module
paths and imports as each feature moves.

The goal is not to remove `App`. A TUI needs an application model. The goal is
to make `App` compose models instead of being every model.

### AR-2: Mutually exclusive overlays are represented by seven independent options

Evidence:

- `App` stores independent `Option` fields for palette, brain input, confirm,
  link picker, assignee filter, help, and sync log.
- [`event_loop/modal_route.rs`](../../src/tui/event_loop/modal_route.rs) describes
  them as "mutually exclusive in practice", converts them back into seven
  booleans, and uses precedence to select one.
- The brain-search picker owns an additional palette and confirmation pair, so
  overlay precedence is split between the global event loop and
  `search_view.rs`.

Impact:

- Multiple overlays can be present even though the UI contract allows one.
- Precedence hides stale state instead of making it unrepresentable.
- Every new overlay requires fields, routing booleans, draw branches, and close
  logic in several places.

Decision:

Introduce one `Overlay` enum with data-carrying variants. Move search overlays
to the shell overlay layer. Route and draw by matching the same enum. This makes
the one-overlay invariant structural.

### AR-3: The two command palettes duplicate application-level actions

Evidence:

- Brain search uses `menu::Choice`; tasks and logs use
  `tui::palette::PaletteAction`.
- Both define and independently dispatch actions such as message brain and
  toggle receiver.
- Search dispatch lives in `search_view.rs`; tasks/log dispatch lives in
  `app_actions/commands.rs`.
- The project instructions require global rows to be kept in both palette
  systems, which records the duplication as an ongoing coordination cost.

Impact:

- A global command can have different labels, visibility, shortcut metadata, or
  behavior depending on which main view opened the palette.
- Adding a global action requires edits in two action enums, two catalogs, two
  dispatchers, tests, and docs.

Decision:

Create a shared `GlobalAction` and one application-level executor. Surface
specific actions wrap or coexist with it. Use one reusable palette state and row
model, with each main view supplying contextual rows and availability. Search
entry actions and task-specific actions remain owned by their features.

### AR-4: Incoming receiver work has a policy but no internal owner

Evidence:

- The shared server side is explicit: `DispatchPipeline` orders workspace
  resolution, provider config, authentication, actor resolution, job building,
  authority revalidation, and forwarding.
- TUI admission takes `&mut Vec<InboundJob>` directly in
  [`tui/singleton.rs`](../../src/tui/singleton.rs).
- Queue consumers use `first`, indexing, `remove(0)`, `split_off`, `push`, and
  `pop` across socket admission, receiver dispatch, receiver control, and
  completion modules.
- `tick_receiver` coordinates completion, activity sampling, timeout,
  lease expiry, socket polling, restart/new controls, sync freshness, panel
  replacement, attachment staging, launch, retry, and active-turn state.
- Receiver invariants are spread across more than a dozen TUI production files
  and more than 30 `App` fields.

Impact:

- Queue invariants are conventions shared by unrelated callers.
- It is hard to tell which state combinations describe idle, interactive,
  dispatching, active remote, warm receiver, retry, or stalled phases.
- Tests must construct large portions of `App` to exercise receiver decisions.
- A raw `Vec` exposes operations the queue contract does not intend.

Decision:

Keep the queue bounded, in memory, and owned by the live TUI. Introduce:

- `InboundQueue`, backed by `VecDeque`, with capacity, staged admission,
  rollback, head inspection, commit, and control-command operations.
- `ReceiverRuntime`, with private queue, socket, enablement, session, active
  turn, timing, and sync-gate state.
- Explicit receiver decisions and effects. Pure state transitions decide what
  should happen; the application mediator performs agent launch, filesystem,
  sync, and provider-delivery effects.

No durable queue or headless receiver is part of this refactor.

### AR-5: TUI startup and lifetime have no owning runtime facade

Evidence:

- `run_tui` takes 12 arguments, including an unused `_with_receiver` argument.
- It acquires the singleton, refreshes hooks, syncs skills, binds the job
  socket, registers the server lease, resolves assignment, configures the
  terminal, opens the state DB, constructs `App`, starts the agent panel,
  launches sync work, creates watchers, runs the event loop, and then manually
  tears down resources.
- `App::new` takes 22 arguments and also performs filesystem, config, task,
  receiver, environment, and signal cleanup work.
- Supporting resources such as singleton guards, heartbeat workers, job
  sockets, watchers, and periodic pullers already have useful RAII behavior.
  Terminal restoration, controller shutdown, and session-lock release remain
  manually sequenced in `run_tui`.
- The event loop handles recurring services, rendering, terminal acquisition,
  modal precedence, global shortcuts, main-view routing, and panel routing in
  one loop.

Impact:

- Construction policy and runtime ownership are difficult to test or change in
  isolation.
- Adding an early return or a new resource requires auditing teardown order.
- `App` cannot be constructed as a clear model because construction performs
  startup work.
- Event handling rules grow in one precedence ladder.

Decision:

Add three explicit boundaries:

1. `TuiLaunch`, an owned request assembled by the tasks command. It removes the
   unused parameter and replaces the long argument list.
2. `TerminalSession`, an RAII owner of raw mode, alternate screen, mouse mode,
   keyboard enhancement mode, cursor restoration, and the ratatui terminal.
3. `TuiRuntime`, the composition root that owns `App`, terminal, singleton,
   server lease, watcher, periodic puller, and the shell session lock. It
   exposes `run`, `tick`, and orderly shutdown.

The terminal event loop should acquire an event, call an application update
boundary, and draw. Recurring receiver, sync, skill-session, triage, and
heartbeat work should enter through a named tick coordinator rather than being
listed directly in the terminal loop.

### AR-6: `App` borrows the complete task CLI for a narrow runtime need

Evidence:

- `App<'a>` retains `&'a tasks::cli::Cli`.
- Runtime uses are limited to rebuilding task views and rendering filter chips.
- The lifetime parameter propagates through every `App` method, handler,
  renderer, and helper.

Impact:

- A parser DTO becomes long-lived application state.
- Lifetimes add noise across the complete TUI API.
- Tests need a CLI value even when exercising unrelated state.

Decision:

Create an owned `TaskViewOptions` model containing only the filters and display
choices the open TUI needs. `App` owns it and no longer has a lifetime
parameter. CLI parsing remains at the command boundary.

### AR-7: Phase 1 produced numbered test fragments instead of semantic modules

Evidence:

- The line-count audit is clean, but the tree contains 89 files named
  `part_01.rs`, `part_02.rs`, and so on.
- Their parent suites use `include!` to concatenate numbered chunks into one
  module.
- Several chunks cross behavior boundaries because they were split by size.

Impact:

- A developer cannot predict which file owns a test from its behavior.
- Inserting or moving tests makes numbering less meaningful.
- The structure meets a numeric limit without satisfying the repository rule
  that modules be split on real seams.

Decision:

Replace numbered fragments with named behavior modules or named included
sections. Group by behavior, subsystem, lifecycle phase, or invariant. Shared
fixtures remain in focused support modules. This is the first implementation
task because it corrects the residual Phase 1 modularity debt without changing
production behavior.

## Medium-priority findings and decisions

### AR-8: Agent frontend architecture is sound; transport viewport concerns are mixed

What is strong:

- `AgentController` is a real facade used by TUI and receiver flows.
- `AgentFrontend` is private and translates semantic actions.
- Claude, Codex, and OpenCode are concrete private adapters.
- The exhaustive registry centralizes construction, command metadata,
  lifecycle installation, health checks, capability evidence, and compatibility
  probes.
- Architecture tests prevent production callers outside `agent` from naming
  concrete frontend types.

Concern:

- `AgentTransport` combines process/input lifecycle with terminal rendering,
  resize, scrolling, and `vt100` access. The controller therefore exposes both
  agent semantics and viewport controls.

Decision:

Preserve the facade, frontend trait, registry, and three adapters. Do not
redesign frontend support in this refactor. Once the TUI panel aggregate exists,
reassess whether terminal viewport behavior belongs in a separate
`TerminalViewport` capability. There is only one production transport today,
so splitting it now would add abstraction before a second implementation needs
it.

### AR-9: Command classification is explicit but repeated

Evidence:

- `invocation_for(&Cli)` classifies the command.
- `bootstrap_policy(invocation)` derives required authority.
- Bootstrap recomputes classification at several phases.
- Dispatch recomputes it, dynamically validates that `BootstrapContext` matches,
  uses a sequence of early command branches, then ends with another exhaustive
  match containing unreachable variants.

Decision:

The current enum-based routing is idiomatic and understandable. Do not replace
it with a dynamic command registry. A future focused refactor should create one
typed `InvocationPlan` immediately after parse and pass it through bootstrap and
dispatch. This is deferred until after the TUI work because it has lower change
cost and no coupling to the TUI model.

### AR-10: Sync is a long transaction script, but its seams are credible

Evidence:

- `sync_once_with_task_state` is a long orchestration function.
- It coordinates identity, safety markers, rclone, auto-recovery, conflicts,
  semantic CSV merge, counters, reporting, and journaling.
- The decision logic and external mechanisms are already split into focused
  modules, and the orchestration order is the central value of the function.

Decision:

Keep the transaction-script pattern. Extracting a class-shaped coordinator
would mostly redistribute local variables. Revisit only when a new sync lane or
new recovery path makes the current ordered flow materially harder to follow.
Correct the module documentation that calls the orchestrator "thin" if it is
touched later.

### AR-11: Shared server HTTP ownership is clear; control workers are not bounded

What is strong:

- Pure route classification, workspace route tickets, `DispatchPipeline`, and
  fixed HTTP workers provide clear boundaries.
- Process ownership and most worker handles use RAII or process-lifetime owners.

Concern:

- `ControlListener::drain` starts one detached thread per accepted control
  connection. The HTTP side deliberately has a fixed `HttpWorkers` owner, while
  control work has no equivalent bounded owner or join set.

Decision:

Defer this from the initial TUI spec. Track a follow-up `ControlWorkers` design
that reuses the fixed-worker ownership pattern and preserves concurrent
revocation behavior. This is a server-runtime concern, not a prerequisite for
the TUI boundary.

### AR-12: The library surface is broader than the product API

Evidence:

- `lib.rs` publicly exports 33 top-level modules so the binary and integration
  tests can share one compiled graph.
- Integration tests directly import many internal modules.
- Several architecture tests inspect exact source files and substrings. One
  such test failed during Phase 1 solely because a call moved to a child module.

Decision:

Keep the single library graph. Do not create a second private module graph or a
large test-support API during the TUI refactor. Replace exact-file source scans
with directory-wide boundary checks or behavior tests when those tests are
touched. A later packaging pass can introduce a narrow `brain::app` entry point
and document which exports are intentionally stable.

## Accepted architecture to preserve

The following areas passed the adversarial review and should not be rewritten
for style alone:

- Thin `main` entry point followed by workspace bootstrap and command dispatch.
- Immutable `WorkspaceContext`, `CommandContext`, and `ActorContext` propagated
  through ordinary work.
- `AgentController` facade, private frontend adapters, exhaustive registry, and
  frontend-neutral lifecycle/completion schema.
- Server receiver `DispatchPipeline` and workspace route ticket pattern.
- Live-TUI-only, bounded, non-durable receiver policy.
- Fixed HTTP workers and provider delivery queue kept off the TUI event loop.
- Pure decision helpers beside thin filesystem, terminal, process, and network
  shells.
- Transaction-script orchestration for sync, backed by focused pure and impure
  modules.
- Small external dependency set. No new crate is required for the refactor.

## Phase 3 implementation boundary

The implementation spec and plan should cover the following, in order:

1. Replace numbered test fragments with behavior-named modules.
2. Introduce owned `TaskViewOptions` and `TuiLaunch`; remove the `App` lifetime
   and long startup signatures.
3. Introduce a single shell `Overlay` enum and move search overlays to it.
4. Introduce shared `GlobalAction`, one global executor, and a reusable palette
   row/state model.
5. Introduce `InboundQueue` and `ReceiverRuntime`, then move receiver state and
   transitions behind them.
6. Introduce `TerminalSession` and `TuiRuntime`; centralize tick, event update,
   draw, and shutdown ownership.
7. Compose `App` from focused state aggregates, replace TUI wildcard imports,
   and narrow or remove the global Clippy allowances that this architecture no
   longer needs.
8. Update architecture, integration, testing, and decision documentation as
   each boundary lands.

The following are explicitly outside the first implementation spec:

- A durable or offline receiver queue.
- A new agent frontend or a redesign of `AgentFrontend`.
- A generic plugin architecture for main views or commands.
- Sync behavior changes.
- Command `InvocationPlan`, bounded server control workers, and public-library
  packaging. These remain documented follow-ups.

## Completion criteria

The architecture refactor is complete when:

- `App` has no lifetime parameter and composes focused state aggregates.
- Receiver queue and lifecycle state are private to `ReceiverRuntime`.
- Only one overlay can be represented at a time.
- Global palette actions have one definition and one executor.
- `run_tui` accepts one launch request and delegates ownership to `TuiRuntime`.
- Terminal, server lease, session lock, watcher, periodic puller, job socket,
  and agent shutdown have explicit owners with deterministic teardown.
- The terminal loop no longer lists every recurring subsystem directly.
- TUI production modules use explicit imports and no root wildcard re-export
  mesh.
- No numbered test fragments remain.
- `rust-loc`, release tests, release Clippy with warnings denied, privacy checks,
  and documentation checks are green.
