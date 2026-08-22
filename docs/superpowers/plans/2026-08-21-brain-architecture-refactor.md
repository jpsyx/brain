# Brain TUI Architecture Refactor Implementation Plan

> **For agentic workers:** REQUIRED SKILLS: Use
> `subagent-driven-development` to execute tasks, `test-driven-development` for
> every production change, `rust-skills` and `rust-router` for Rust work, and
> `systematic-debugging` before changing characterized behavior in response to
> a regression.

**Goal:** Replace the persistent TUI's flat 96-field application bag and
wildcard namespace with explicit launch, runtime, overlay, action, queue,
receiver, and feature-state ownership while preserving behavior.

**Design:**
[2026-08-21-brain-architecture-refactor.md](../specs/2026-08-21-brain-architecture-refactor.md)

**Architecture review:**
[2026-08-21-architecture-review.md](../2026-08-21-architecture-review.md)

**Starting point:** `82d0da9` on `refactor/arch`, crate version `0.71.3`.

**Tech stack:** Rust 2024, clap, ratatui, crossterm, `VecDeque`, existing
workspace and agent abstractions, and the current test suite. Add no shipped or
runtime dependency.

**Accepted implementation deviation:** The original plan said "Add no
dependency." Direct dev-only `syn` and `proc-macro2` declarations are allowed
for the queue-ownership architecture guard. Both crates were already
transitive dependencies, neither ships in the binary, and the AST/token-tree
guard replaces an unsound handwritten Rust parser. This is an explicit change
to the original acceptance wording, not an assertion that no manifest entry
was added.

## Global constraints

These constraints bind every task and must be copied into each task review:

- Preserve all user-visible behavior. Do not add or change commands,
  keybindings, palette labels, receiver policy, agent behavior, sync behavior,
  task behavior, or terminal presentation.
- Correctness changes are outside scope. Existing tests are characterization.
  If a structural move exposes a behavioral failure, stop and use systematic
  debugging before proposing any behavior change.
- Follow red, green, refactor TDD for every production change. Add the smallest
  architectural or behavioral test, run it and observe the expected failure,
  implement the minimum boundary, then keep the focused and release suites
  green.
- Keep every LLM flow behind `AgentController`; do not redesign
  `AgentFrontend`, the registry, or the Claude, Codex, and OpenCode adapters.
- Keep incoming receiver jobs bounded to 64, in memory, and owned by the live
  workspace TUI. Do not add persistence or headless dispatch.
- Keep pure decisions separate from terminal, filesystem, process, database,
  sync, and provider-delivery effects.
- Add no shipped or runtime dependency. Direct dev-only `syn` and
  `proc-macro2` are the accepted architecture-test exception described above.
- Keep production Rust files under the repository's modularity review
  threshold and split only on semantic seams. Test files and support files are
  subject to the same review.
- Update the relevant durable docs in the same task that changes a module or
  ownership boundary.
- Bump the patch version in `Cargo.toml` and `Cargo.lock` in every commit. This
  includes task-review fix commits. Do not combine separate plan tasks in one
  commit.
- Do not push, merge, or remove the `refactor/arch` worktree. The user will
  review the completed branch.
- Before committing, run `cargo fmt --check`, the task's focused tests, the full
  `cargo test --release`, and `git diff --check`. Run release Clippy when the
  task changes imports, visibility, module boundaries, or public internal APIs.

### Task 1: Replace numbered test fragments with behavior-owned sections

**Files:**

- Create: `tests/module_structure.rs`
- Rename/regroup: every `part_XX.rs` below `src/**` and `tests/**`
- Modify: the parent `include!` declarations for those suites
- Modify: `docs/architecture.md`
- Modify: `docs/testing.md`
- Add: `docs/superpowers/2026-08-21-architecture-review.md`
- Add: `docs/superpowers/specs/2026-08-21-brain-architecture-refactor.md`
- Add: `docs/superpowers/plans/2026-08-21-brain-architecture-refactor.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **RED: guard semantic test filenames.** Add an integration test that
  recursively scans tracked Rust test locations and rejects filenames matching
  `part_<digits>.rs`. The failure must list the offending paths. Run
  `cargo test --release --test module_structure` and observe all current
  fragments fail the guard.

- [ ] Inventory the test functions and helpers in each numbered suite. Choose
  names from behavior or subsystem, such as `navigation.rs`,
  `receiver_controls.rs`, `migration_recovery.rs`, or
  `late_revocation.rs`. Do not merely replace numbers with letters, ordinal
  words, or ranges.

- [ ] Rename with `git mv` semantics and update each parent include list.
  Regroup a chunk when it visibly crosses behaviors. Preserve shared lexical
  scope when the existing `include!` suite relies on common imports or helpers;
  a behavior-named included section is acceptable. Do not force an artificial
  nested module that duplicates fixtures.

- [ ] Keep each resulting file cohesive and below the review threshold. Do not
  alter test assertions or production code except the new structure guard.

- [ ] **GREEN:** run the new guard, all affected suites, then the full release
  suite.

- [ ] Document behavior-owned test layout and the directory-wide architecture
  guard. Include the review, design, and plan documents in this first task so
  the branch records the complete decision trail.

**Exit criteria:** No numbered Rust test fragment exists, a directory-wide test
prevents recurrence, and all test behavior is unchanged.

### Task 2: Introduce owned task options and the TUI launch boundary

**Files:**

- Create: `src/tasks/view/options.rs`
- Create: `src/tui/launch.rs`
- Modify: `src/tasks/cli.rs`
- Modify: `src/tasks/view/mod.rs`
- Modify: `src/tasks/view/build.rs`
- Modify: `src/tasks/render/chrome.rs`
- Modify: `src/command/tasks/browse.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/app_state/construct.rs`
- Modify: `src/tui/app_state/view.rs`
- Modify: `src/tui/app_actions/triage.rs`
- Modify: `src/tui/event_loop/setup/mod.rs`
- Modify: TUI test fixtures and affected tests
- Modify: `docs/architecture.md`
- Modify: `docs/testing.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **RED: runtime options own only runtime data.** Add pure tests for
  `TaskViewOptions::from(&Cli)` covering filters, sort, reverse, and full-notes.
  Prove the options are owned by mutating or dropping the source CLI before
  using the options.

- [ ] **RED: TUI construction has one owned boundary.** Add a compile-time or
  directory-wide architecture test that rejects `App<'...>`, a stored `&Cli`,
  a `run_tui` signature with more than one parameter, and the obsolete
  `with_receiver` parameter. Observe it fail before production edits.

- [ ] Implement `TaskViewOptions`. Update task view building and header
  rendering to accept it. Keep the CLI conversion at the command boundary.
  Do not make clap types the canonical domain model.

- [ ] Implement owned `TuiLaunch`. `command::tasks::browse` moves or clones its
  already-resolved values into the request and calls `run_tui(launch)`.

- [ ] Add one private `AppInit` request assembled by startup after it resolves
  config, assignment, DB, search state, services, and server identity.
  `App::new(AppInit)` initializes model state and keeps existing side effects in
  their current startup owner.

- [ ] Remove the `App` lifetime parameter and update all implementations,
  renderers, handlers, and test fixtures.

- [ ] **GREEN:** run task view tests, TUI construction and view tests,
  architecture tests, full release tests, and release Clippy.

- [ ] Update the architecture and testing docs with the command DTO to runtime
  model boundary and one-request startup seam.

**Exit criteria:** `App` is owned and lifetime-free, stores no clap object,
`run_tui` accepts one `TuiLaunch`, `App::new` accepts one `AppInit`, and behavior
is unchanged.

### Task 3: Make one overlay the only representable modal state

**Files:**

- Create: `src/tui/overlay/mod.rs`
- Move or modify: `src/tui/modal_state.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/picker/mod.rs`
- Modify: `src/picker/filter.rs`
- Modify: `src/picker/selection.rs`
- Modify: `src/picker/view.rs`
- Modify: `src/tui/search_view.rs`
- Modify: `src/tui/event_loop/modal_route.rs`
- Modify: `src/tui/event_loop/run.rs`
- Modify: `src/tui/draw/mod.rs`
- Modify: overlay handlers and tests under `src/tui/`
- Modify: picker tests that construct palette or confirmation state
- Modify: `docs/architecture.md`
- Modify: `docs/decisions.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **RED: express the one-overlay invariant.** Add pure tests for an
  `Overlay` owner that opens, replaces, routes, and closes data-bearing
  variants. Add an architecture check that rejects the current independent
  shell modal options and `picker::App` palette/confirm fields. Observe the
  failures.

- [ ] Introduce `Overlay` variants for the existing task palette, brain input,
  task confirmation, search palette, search confirmation, link picker,
  assignee filter, help, and sync log.

- [ ] Replace the seven shell `Option` fields with one `Option<Overlay>`. Move
  search palette and confirmation ownership out of `picker::App`; picker
  selection helpers return the data needed to construct a shell overlay.

- [ ] Route keys and draw modals by matching the same enum. Remove
  `ActiveModals`, its boolean precedence, and search-view overlay precedence.
  Preserve current captive-modal and escape behavior.

- [ ] Make open/replace/close operations explicit. Code handling a variant may
  temporarily take it, perform the existing operation, and restore it only when
  the handler's `Continue` result requires that.

- [ ] **GREEN:** run picker, modal, draw, search-view, keymap, and full release
  tests plus Clippy.

- [ ] Record the single-overlay decision and update the TUI state/data flow.

**Exit criteria:** only `Option<Overlay>` can represent modal state, picker has
no overlay fields, and no precedence booleans remain.

### Task 4: Unify global actions and palette mechanics

**Files:**

- Create: `src/tui/action/mod.rs`
- Create: `src/tui/action/global.rs`
- Create or modify: `src/tui/palette/model.rs`
- Modify: `src/tui/palette/command.rs`
- Modify: `src/tui/palette/catalog.rs`
- Modify: `src/tui/palette/mod.rs`
- Modify: `src/menu/model/mod.rs`
- Modify: `src/menu/model/tests/**`
- Modify: `src/menu/mod.rs`
- Modify: `src/tui/search_view.rs`
- Modify: `src/tui/app_actions/commands.rs`
- Modify: palette handlers, renderers, and tests
- Modify: `src/tui/overlay/mod.rs`
- Modify: `docs/architecture.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **RED: one identity for application actions.** Add table-driven tests
  proving the task/log and brain-search catalogs emit the same
  `GlobalAction` for every shared application command, with identical labels
  and shortcut metadata where the row is shared.

- [ ] **RED: one palette state contract.** Add pure behavior tests for reusable
  palette rows and state: filtering, empty results, selection clamping,
  movement, number selection, cancellation, and confirmation.

- [ ] Introduce `GlobalAction` containing the existing application-level
  commands. Define feature-owned `SearchAction` and `TaskAction`, or equivalent
  wrappers, for feature-only operations. Do not put entry paths or task IDs in
  global actions.

- [ ] Introduce one reusable `PaletteRow` and `CommandPalette` model. Convert
  both catalogs to build rows through it. Preserve the current row ordering,
  dynamic labels, conditional visibility, direct shortcuts, task context, and
  destructive-row placement.

- [ ] Replace duplicate global dispatch branches with one
  `App::execute_global_action`. Feature executors may call it for a wrapped
  global action. Keep cross-feature effects at the application mediator.

- [ ] Remove duplicated `Msg`, `ToggleReceiver`, layout, task-view navigation,
  and other global identities from the two old enums. Remove old palette state
  code once both surfaces use the shared model.

- [ ] **GREEN:** run menu, palette, search-view, task action, keymap, and full
  release tests plus Clippy.

- [ ] Document shared application actions, contextual catalogs, and the reason
  this is a closed enum rather than a plugin registry.

**Exit criteria:** global actions have one definition and executor, both
palettes use one row/state abstraction, and view-specific actions remain
feature owned.

### Task 5: Encapsulate the bounded incoming queue

**Files:**

- Create: `src/tui/receiver/queue.rs`
- Modify: `src/tui/receiver/mod.rs` or create it if the current receiver module
  shape differs
- Modify: `src/tui/singleton.rs`
- Modify: `src/tui/receiver/policy.rs`
- Modify: receiver dispatch, control, completion, and tests under
  `src/tui/app_brain/receiver/**`
- Modify: socket and receiver tests
- Modify: `docs/architecture.md`
- Modify: `docs/integrations.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **RED: specify the queue contract.** Add pure tests for FIFO ordering,
  the capacity-64 boundary, staged append, successful finalization, exact-tail
  rollback after acknowledgement failure, head commit, controls that remove or
  retain jobs, and snapshots used by tests.

- [ ] **RED: reject representation leaks.** Add a directory-wide architecture
  test that rejects `Vec<InboundJob>` and direct `push`, `pop`, indexing,
  `remove(0)`, or `split_off` queue operations outside `queue.rs`. Observe it
  fail on the existing callers.

- [ ] Implement `InboundQueue` with `VecDeque`. Keep capacity private and
  expose semantic operations only. Use an opaque staged-admission token so a
  failed final acknowledgement can roll back only the job it staged.

- [ ] Change `JobSocket::poll_jobs` and its stream helper to accept
  `&mut InboundQueue`. Preserve the protocol ordering: validate, stage, send
  accepted acknowledgement, finalize; roll back exact staged work if the
  acknowledgement write fails.

- [ ] Convert every receiver consumer to the semantic queue surface. Tests may
  use a read-only snapshot or iterator but may not get mutable representation
  access.

- [ ] **GREEN:** run singleton/socket tests, receiver state and integration
  tests, architecture tests, full release tests, and Clippy.

- [ ] Document queue ownership, capacity, socket transaction, and why the queue
  remains live-TUI-only.

**Exit criteria:** only `InboundQueue` owns queue representation and capacity,
all queue mutations are semantic, and acknowledgement rollback is structurally
scoped to the staged job.

### Task 6: Make ReceiverRuntime own receiver-local state

**Files:**

- Create: `src/tui/receiver/runtime.rs`
- Create or modify: `src/tui/receiver/mod.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/app_state/construct.rs`
- Modify: `src/tui/app_brain/receiver/**`
- Modify: `src/tui/receiver/policy.rs`
- Modify: `src/tui/singleton.rs`
- Modify: receiver-related TUI tests and fixtures
- Modify: `docs/architecture.md`
- Modify: `docs/integrations.md`
- Modify: `docs/testing.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **RED: define receiver ownership.** Add construction and semantic-method
  tests for a `ReceiverRuntime` that owns queue, socket, intent, channel
  controls, lease, generation, sender/recipient context, interactive and remote
  session identity, activity sampling, retry timing, and sync-gate state.

- [ ] Add an architecture check that lists the receiver field prefixes and raw
  types no longer permitted on `App`, and that rejects direct receiver-runtime
  field access outside `src/tui/receiver/`. Observe it fail before moving
  fields.

- [ ] Move receiver-local fields from `App` into `ReceiverRuntime`. Keep fields
  private. Give the runtime semantic operations and focused read-only queries
  derived from existing use cases. Do not create a getter and setter for every
  old field.

- [ ] Move receiver-local methods that do not require task, shell, brain panel,
  DB, filesystem, sync, or provider effects onto `ReceiverRuntime`. Keep
  cross-feature orchestration on `App` for Task 7.

- [ ] Update fixtures to build one receiver runtime. Use focused fixture
  builders rather than growing a catch-all fake runtime.

- [ ] Remove temporary flat-field compatibility methods once all callers use
  semantic runtime operations.

- [ ] **GREEN:** run all receiver state, control, completion, sync, and agent
  receiver tests, architecture tests, full release tests, and Clippy.

- [ ] Update receiver ownership diagrams and testing seams.

**Exit criteria:** `App` owns one `ReceiverRuntime`, all receiver fields are
private to its module, and other TUI code uses semantic methods.

### Task 7: Separate receiver decisions from application effects

**Files:**

- Create: `src/tui/receiver/decision.rs`
- Create: `src/tui/receiver/effect.rs`
- Modify: `src/tui/receiver/runtime.rs`
- Modify: `src/tui/app_brain/receiver/dispatch.rs`
- Modify: receiver completion, activity, control, sync, and state modules
- Modify: receiver transition and integration tests
- Modify: `docs/architecture.md`
- Modify: `docs/integrations.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] Inventory the ordered stages in the existing `tick_receiver` and map
  each to receiver-local inputs, decisions, state changes, and external
  effects. Preserve the current order as a characterized contract.

- [ ] **RED: pure tick planning.** Add table-driven tests for each existing
  lifecycle condition: disabled, idle, queued, waiting on sync freshness,
  eligible to dispatch, interactive turn busy, active remote turn, activity
  probe due, timeout/delay notice due, completion available, lease expiry,
  retry waiting, restart requested, and new-session requested.

- [ ] Introduce focused receiver decision values and an effect enum derived
  only from actual existing stages. Do not force independent timers into one
  combinatorial state enum.

- [ ] Make `ReceiverRuntime` decide due work and update receiver-local state.
  Keep execution of agent launch, controller input, panel replacement,
  attachment staging, sync freshness, filesystem, and provider delivery on the
  `App` mediator or an application-owned effect executor.

- [ ] Feed effect results back through semantic receiver events or methods.
  A receiver module must not gain mutable access to unrelated `App` aggregates.

- [ ] Replace the monolithic `tick_receiver` body with a readable coordinator
  that asks for a plan, executes named effects in existing order, and records
  results. Do not change timeouts, retry schedules, delivery semantics, or
  panel behavior.

- [ ] **GREEN:** run pure decision tests, complete receiver and all-frontend
  receiver suites, full release tests, and Clippy.

- [ ] Document the decision/effect boundary and the reason Brain does not use a
  single giant receiver state enum.

**Exit criteria:** receiver-local transitions are pure and tested,
cross-boundary work is explicit effects owned by the application mediator, and
the terminal loop knows no receiver details.

### Task 8: Add TerminalSession as the terminal RAII owner

**Files:**

- Create: `src/tui/runtime/terminal.rs`
- Create or modify: `src/tui/runtime/mod.rs`
- Modify: `src/tui/event_loop/setup/mod.rs`
- Modify: event-loop setup tests
- Modify: `docs/architecture.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **RED: specify lifecycle planning.** Add headless tests for ordered
  terminal acquisition rollback, normal restoration, restoration after an
  event-loop error, optional keyboard-mode cleanup, and idempotent repeated
  restoration. Use a pure operation plan or a small injected terminal-ops seam;
  tests must not open `/dev/tty`.

- [ ] Implement `TerminalSession::acquire` to own the `/dev/tty` file,
  ratatui terminal, raw mode, alternate screen, mouse capture, mouse-motion
  adjustment, keyboard enhancement state, and cursor restoration.

- [ ] Implement idempotent `restore` with deterministic reverse cleanup.
  `Drop` performs best-effort restoration and logs failures without panicking.
  A failed acquisition step rolls back modes already enabled.

- [ ] Replace manual terminal setup and teardown in `run_tui` with the guard.
  Keep the event loop taking a mutable ratatui terminal through a narrow method.

- [ ] Preserve `/dev/tty` rendering and current mode sequences exactly. Do not
  redirect output to stdout or add terminal capability behavior.

- [ ] **GREEN:** run terminal lifecycle and event-loop setup tests, full
  release tests, and Clippy.

- [ ] Document terminal ownership, cleanup guarantees, and the headless test
  seam.

**Exit criteria:** one RAII guard owns terminal modes and restoration, every
return path restores best-effort, and setup no longer sequences terminal
cleanup manually.

### Task 9: Add TuiRuntime and simplify the event loop

**Files:**

- Create: `src/tui/runtime/tick.rs`
- Create: `src/tui/runtime/shutdown.rs`
- Modify: `src/tui/runtime/mod.rs`
- Modify: `src/tui/event_loop/setup/mod.rs`
- Modify: `src/tui/event_loop/run.rs`
- Modify: `src/tui/event_loop/mod.rs`
- Modify: runtime, event-loop, triage, sync, and skill-session tests
- Modify: `docs/architecture.md`
- Modify: `docs/integrations.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **RED: specify runtime ownership and teardown.** Add pure lifecycle tests
  proving ordered acquisition state, idempotent shutdown, shared-server lease
  stop before agent shutdown, watcher and periodic worker drop, session-lock
  release, terminal restoration, and singleton held until runtime drop.

- [ ] **RED: specify one tick coordinator.** Add tests that characterize the
  current recurring order for receiver, skill sessions, triage gate, logical
  day, sync status, heartbeat/server health, and task refresh. Test decisions
  without sleeping or opening a terminal.

- [ ] Implement `TuiRuntime::start(TuiLaunch)`. Move singleton acquisition,
  hook/skill refresh, job socket binding, server registration, assignment,
  terminal acquisition, DB/config/model initialization, initial agent panel,
  startup sync, watcher, and periodic puller into clearly named builder stages.

- [ ] Make the runtime own `App`, `TerminalSession`, singleton guard,
  heartbeat worker, watcher, periodic puller, and shell instance/session lock.
  Reuse existing RAII types rather than wrapping their internals.

- [ ] Implement one named recurring tick coordinator. It calls existing
  feature boundaries in characterized order. The terminal loop calls `tick`,
  draw, poll/read, and one application event-update function; it no longer
  lists each recurring subsystem.

- [ ] Implement idempotent orderly shutdown and best-effort `Drop`. Preserve
  current teardown order and error logging. `run_tui` becomes a thin facade
  that starts and runs the runtime.

- [ ] **GREEN:** run runtime, event-loop, receiver, triage, sync, skill-session,
  and full release tests plus Clippy.

- [ ] Document startup stages, runtime ownership, event flow, and deterministic
  teardown.

**Exit criteria:** `TuiRuntime` owns all process-lifetime resources, `run_tui`
is thin, recurring work has one coordinator, and the event loop contains only
runtime tick/draw/input/update structure.

### Task 10: Move task and shell state behind focused aggregates

**Files:**

- Create: `src/tui/state/mod.rs`
- Create: `src/tui/state/tasks.rs`
- Create: `src/tui/state/shell.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/app_state/**`
- Modify: `src/tui/draw/**`
- Modify: `src/tui/event_loop/**`
- Modify: `src/tui/search_view.rs`
- Modify: `src/tui/app_actions/**`
- Modify: task, shell, draw, keymap, palette, and search tests
- Modify: `docs/architecture.md`
- Modify: `docs/testing.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] Inventory fields and methods that are wholly task-list state versus shell
  navigation state. Keep immutable workspace/runtime identity out of both.

- [ ] **RED: focused models stand alone.** Add construction and pure behavior
  tests for `TasksState` covering view data, selection, query/filter,
  assignment, notes, body layout, and scrolling. Add tests for `ShellState`
  covering main view, focus, panel side/rect, search picker, logs view, and
  active tab selection.

- [ ] Move task-owned fields into `TasksState` and feature-local methods into
  its module. Expose semantic operations needed by renderers and mediator code;
  do not reproduce the flat bag with one accessor per field.

- [ ] Move shell navigation and embedded search fields into `ShellState` and
  move local navigation operations with them. Keep actions requiring DB,
  controller, receiver, or task mutation on `App`.

- [ ] Update renderers and handlers to accept focused state or call semantic
  `App` coordination methods. Avoid passing the whole `App` when a renderer
  needs only one aggregate.

- [ ] Remove old flat fields and temporary forwarding accessors. Add an
  architecture assertion that the moved field set exists only in the owning
  aggregate.

- [ ] **GREEN:** run task view/render, shell navigation, picker/search, palette,
  keymap, mouse, and full release tests plus Clippy.

- [ ] Update the TUI component map and pure-state testing guidance.

**Exit criteria:** task and shell invariants each have one focused owner,
renderers consume focused state, and `App` coordinates only cross-feature work.

### Task 11: Finish App composition with context, brain, services, and status

**Files:**

- Create: `src/tui/state/context.rs`
- Create: `src/tui/state/brain.rs`
- Create: `src/tui/state/services.rs`
- Create: `src/tui/state/status.rs`
- Modify: `src/tui/state/mod.rs`
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/app_brain/**`
- Modify: `src/tui/app_state/**`
- Modify: `src/tui/app_actions/**`
- Modify: `src/tui/draw/**`
- Modify: TUI fixtures and affected tests
- Modify: `docs/architecture.md`
- Modify: `docs/integrations.md`
- Modify: `docs/testing.md`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] Inventory the remaining `App` fields and assign each to immutable
  `AppContext`, `BrainPanelState`, injected `AppServices`, transient
  `StatusState`, existing `ReceiverRuntime`, existing `Overlay`, or a justified
  top-level mediator field. A justified mediator field must be documented.

- [ ] **RED: characterize each aggregate.** Add focused construction and state
  tests for brain controller/tab/session ownership, immutable context identity,
  injected service access, and flash/warning/alert/sync status. Add an
  architecture guard with a maximum top-level `App` field count matching the
  final design, expected to be no more than ten.

- [ ] Move main controller, turn activity, skill-session tabs, session actors
  and IDs, instance identity, and test transport seams into
  `BrainPanelState`. Move feature-local state operations with it, but keep
  launch, DB registration, receiver takeover, and cross-feature focus changes
  on the mediator.

- [ ] Move immutable command/workspace/config/frontend/path/day data into
  `AppContext`; move runners, DB, intent refresher, and receiver sync runtime to
  `AppServices`; move flash, warnings, alert, and sync status to `StatusState`.
  If a service belongs naturally to `ReceiverRuntime` after Task 6, keep it
  there instead of moving it twice.

- [ ] Rewrite `App` as a small composition root over focused aggregates. Keep
  mediator methods cohesive and place feature-local methods on their feature
  types. Remove all flat compatibility accessors.

- [ ] Update fixtures to compose focused builders. Do not introduce a single
  catch-all fake with knowledge of every feature.

- [ ] **GREEN:** run all agent, skill-session, receiver, overlay, status, draw,
  event-loop, and full release tests plus Clippy.

- [ ] Update application composition and agent/receiver integration diagrams.

**Exit criteria:** `App` has at most ten intentional aggregate or mediator
fields, each mutable invariant has one owner, and the agent facade remains the
only frontend entry point.

### Task 12: Enforce explicit TUI dependencies and complete verification

**Files:**

- Modify: `src/tui/mod.rs`
- Modify: production modules under `src/tui/**`
- Modify: test modules that rely on TUI-root wildcard imports
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `tests/module_structure.rs` or create a focused architecture test
- Modify: `docs/architecture.md`
- Modify: `docs/integrations.md`
- Modify: `docs/decisions.md`
- Modify: `docs/testing.md`
- Modify when required by discovered stale text: `docs/glossary.md`

- [ ] **RED: enforce the final module boundary.** Add directory-wide checks
  that reject production `use crate::tui::*`, `use super::*` used to obtain
  sibling production APIs, and wildcard child re-exports from `tui/mod.rs`.
  Check that `App` is lifetime-free, the overlay and receiver fields have one
  owner, `run_tui` has one request, and numbered fragments remain absent.

- [ ] Replace TUI production glob imports with explicit owning-module paths and
  named imports. Reduce visibility from `pub(crate)` where an item is now owned
  by one TUI subtree. Keep `tui/mod.rs` as a thin module declaration and
  intentional entry-surface file.

- [ ] Remove global Clippy allowances for `wildcard_imports` and
  `redundant_pub_crate` when the refactor makes them unnecessary. If an
  unrelated test-only use remains, move the allowance to the narrowest module
  and explain the local reason. Do not preserve an obsolete TUI comment.

- [ ] Run `rust-loc` and inspect every file near or above the repository's
  modularity threshold. Split only along a real behavior or ownership seam.

- [ ] Audit all design acceptance criteria and the architecture review's
  accepted boundaries. Confirm no concrete frontend escaped `src/agent`, no
  receiver persistence appeared, and no sync or command-routing redesign
  slipped in.

- [ ] Bring durable docs to the final code shape. Remove interim terminology,
  correct module lists and data-flow diagrams, and add the final architecture
  guards to testing docs.

- [ ] **Full verification:** run all of the following with pristine output:

```sh
cargo fmt --check
rust-loc
cargo test --release
cargo clippy --release --all-targets -- -D warnings
cargo test --release bundled_skills_carry_no_personal_data
python3 -m unittest discover -s skills/todo/scripts/tests
cargo test --release --test workspace_docs
git diff --check
```

- [ ] Inspect `git status`, the complete branch diff from `82d0da9`, and the
  commit list. Do not push or merge.

**Exit criteria:** all design acceptance criteria and repository gates pass,
the TUI dependency graph is explicit, documentation matches code, and the
branch is ready for the user's review.

## Final review protocol

After every task, run the subagent-driven-development task review and resolve
all Critical, Important, and spec-compliance findings before starting the next
task. After Task 12, dispatch a fresh strongest-model reviewer across
`82d0da9..HEAD` using the full design and this plan. Fix all blocking findings,
re-run the full verification block, and perform one final scoped re-review.

Leave the completed commits on `refactor/arch`. Do not push, merge, remove the
worktree, or update `main` without a new user instruction.
