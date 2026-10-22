# Task mutation queue implementation plan

Design: [Optimistic task mutations](../specs/task-mutation-queue.md).

## Global Constraints

- One FIFO native service serializes TUI completion and all deletion requests.
- Confirmed mutations close their modals and update the list immediately.
- Canonical rows and per-job optimistic effects remain separate; every refresh
  reapplies pending effects and rollback removes only the failed job's effect.
- Stable UUID identity and pinned workspace/actor prevent stale IDs from
  redirecting writes. Already-complete habits never spawn twice.
- Only worker-side lock acquisition retries contention. Never replay a mutation
  that may have committed or invoked an external action.
- Preserve deletion consent, Cancel default, `d`, policy choices, durable
  receipts, and explicit recovery. Preview waiting is clearly read-only.
- Committed changes remain committed despite downstream warnings or reload
  errors. Every accepted result is accounted for even after its modal closes.
- Normal quit cannot discard accepted in-memory jobs; input and rendering remain
  live while they wait. No durable outbox or unrelated command rewrite.
- Red/green TDD precedes each production behavior. No unsafe code, dependencies,
  personal data, agent handoffs, production services, or unrelated refactors.

## Task 1: Implement the queue and optimistic TUI end to end

Read the design at `docs/superpowers/specs/task-mutation-queue.md`; it supplies
the behavior and failure contract. Implement this cohesive feature in the
existing native boundaries, using focused child modules rather than a large
coordinator file.

Primary seams:

- `src/tasks/store_lock.rs`: queued worker acquisition policy, with bounded CLI
  behavior unchanged. Retry only typed busy/locked errors before ownership.
- `src/tasks/complete/`: UUID-aware queued completion, idempotent done occurrence,
  local persistence outcome separate from post-commit work. Reuse existing domain
  transformations and owner validation, preserving CLI behavior.
- `src/tasks/removal/`: thread queued acquisition through preview, apply, and
  recovery including reacquisition after external work. Keep fingerprint and
  durable receipt semantics intact.
- `src/tui/task_mutation/` (new): shared requests/results, FIFO worker,
  per-job coordination, and pure optimistic projection. Split worker/native
  execution/model/projection/test responsibilities as appropriate; existing
  removal worker code can move or delegate here instead of being duplicated.
- `src/tui/task_removal/`: use the shared service; separate latest visible
  preview from all accepted jobs/recovery records. Close on accepted Apply,
  keep failure/recovery accessible without overwriting a newer modal.
- `src/tui/state/tasks/`, `state/services/`, `app_state/view.rs`: authoritative
  rows plus projected rows for views, task pickers, and logical task state.
- `src/tui/app_actions/commands.rs`, completion confirmation state/handler,
  `runtime/tick.rs`, `event_loop/run.rs`: enqueue completion by captured
  identity, drain results without blocking, centrally guard all quit routes.
- Status rendering: show pending accepted changes independently of transient
  flashes and retain actionable errors/recovery notices.

Follow red/green in small steps. Use debug focused tests during iteration, since
release test recompilation is expensive. The test evidence must include:

1. Pure FIFO settlement/projection: two pending changes, fail one, preserve the
   other and an intervening authoritative edit; no old snapshot restoration.
2. Completion confirmation closes immediately under a real held task-store lock,
   projects done state, accepts navigation, and persists after lock release.
3. Deletion Cancel/Enter remains safe; preview copy is read-only; a ready preview
   confirmed with `d` immediately closes and hides its target while lock waits.
4. Completion and deletion use the same FIFO and all accepted results settle,
   including while another modal is open; a queued preview observes predecessors.
5. Refresh and view switching preserve pending effects; committed refresh errors
   do not revive removed tasks or roll back done state.
6. Failure restores only its own effect and reports identity/reason; stale
   deletion consent requires reconfirmation; warnings and durable commits remain
   distinct from failures; recovery metadata survives later jobs.
7. Captured UUID survives ID reassignment or rejects disappearance, confirmation
   cannot drift after refresh, and duplicate/already-done habit requests cannot
   create multiple next occurrences.
8. Typed lock contention can continue waiting without replaying mutations;
   worker disconnect is visible; all quit routes guard accepted jobs but allow
   exit once settled and do not block solely on read-only previews.

Adapt affected existing tests to the asynchronous contract, preserving their
original behavioral assertions. Keep tests cohesive and meaningful; do not
weaken architecture guards to bypass a broken ownership boundary. Use actual
temporary files for native operations, never the user's task stores.

Update relevant `docs/architecture.md`, `docs/features.md`, `docs/data-model.md`,
`docs/decisions.md`, `docs/testing.md`, and keybinding quit/deletion descriptions
if their behavior changed. Keep this spec aligned with final implementation.
No new shortcut or palette action is required merely to queue existing commands;
if adding an action for recovery, honor full shortcut/palette/CLI parity.

Run focused tests throughout, then full `cargo test --release`,
`cargo clippy --release --all-targets -- -D warnings`, and `cargo fmt --check`
once the feature is complete. Review the diff for personal data. Bump the crate
and lockfile to `0.104.0` and commit the feature (including the supplied design
and plan) in this worktree. Do not push or install; the controller owns final
review, publication, and local installation. Any necessary follow-up commit
must bump the patch version again.

Write the implementation report to the supplied report path, including exact
RED and GREEN evidence, final validation results, files changed, and limitations.
Return only status, commit IDs, test summary, concerns, and report path. If a
design choice cannot satisfy the safety contract, ask the controller rather
than silently weakening it.
