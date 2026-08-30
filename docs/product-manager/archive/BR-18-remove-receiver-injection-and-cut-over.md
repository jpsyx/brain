---
id: BR-18
title: Remove receiver injection and complete the cutover
status: done
priority: high
assignee: jpsyx
labels: [tech-debt, server]
estimate: 13
project: PROJ-1
milestone: MS-4
cycle:
parent:
github:
blocked_by: []
created: 2026-08-23
updated: 2026-08-29
---

# BR-18: Remove receiver injection and complete the cutover

## Description

Make the durable per-message path the only receiver execution path. Remove
warm-panel receiver reuse, receiver text injection, main-panel takeover,
process-local queue authority, coarse screen-based abandonment, and state that
exists only to support those behaviors. Preserve generic interactive
`AgentController` operations that still have non-receiver callers.

Complete automatic migration, restart reconciliation, diagnostics, user-facing
status, documentation, and full verification. The finished system must expose
whether a job is queued, running, recovering, awaiting delivery, failed, or
done without leaking private message contents.

## Acceptance criteria

- [x] No receiver code injects text or submit keys into an existing Claude,
      Codex, OpenCode, or main-panel process.
- [x] The in-memory `VecDeque` and warm receiver lease are no longer queue or
      conversation authority.
- [x] Legacy receiver state migrates automatically with tested `up` and `down`
      operations; help and version remain side-effect free.
- [x] Receiver status and logs report durable queue depth, active job phase,
      recovery attempt, and delivery state using themed, redacted output.
- [x] Shutdown and restart at each lifecycle phase leave jobs recoverable and
      never block later jobs indefinitely.
- [x] Existing receiver routing, authorization, control commands, attachments,
      sync freshness, completion push, task reload, and response behavior remain
      supported or are deliberately redefined in docs and tests.
- [x] The obsolete BR-10 watchdog and old receiver dispatch/completion tests are
      removed or rewritten around the durable lifecycle rather than retained as
      a dormant parallel path.
- [x] All applicable files under `docs/` are updated in the same change,
      including architecture, features, integrations, decisions, data model,
      glossary, keybindings, config, and testing where relevant.
- [x] `cargo test --release` passes.
- [x] `cargo clippy --release --all-targets -- -D warnings` passes.

## Notes

### Pointers (as of 2026-08-29)

- `src/state/receiver/schema.rs`, `schema/job_contract.rs`,
  `schema/delivery/`, and `store/reconciliation/` now own schema v12, durable
  answer delivery, and terminal-notice conversion. The remaining
  `pending_unavailable_notice` and notice-claim columns are the last runtime
  bridge to remove through a reversible schema-v13 migration.
- `src/tui/singleton.rs`, `src/tui/runtime/{builder,shutdown}.rs`,
  `src/server/control/`, `src/server/lifecycle/lease.rs`, and
  `src/server/workspace_route.rs` retain `jobs.sock` only as a live endpoint
  marker. No receiver job crosses it; remove the representation without
  weakening the heartbeat lease, workspace route, or local-capability checks.
- `src/tui/app_brain/receiver/`, `src/tui/receiver/`, and
  `tests/tui_receiver_{dispatch,runtime}_architecture*` contain the durable
  coordinator and structural no-injection guards. Keep transient effect state
  and generic interactive controller APIs, while proving neither is queue or
  conversation authority.
- `src/command/server/receiver/{enablement,details}.rs` and
  `src/state/receiver/store/delivery/status.rs` expose only delivery counts
  today. Extend the read-only state summary and stable logs with durable queue,
  active phase, recovery attempt, and delivery state without message content.
- `docs/README.md` and the repository AGENTS docs-contract table identify all
  architecture, feature, integration, data-model, decision, glossary,
  keybinding, config, and testing documents that require audit during the
  final cutover.

### Plan (2026-08-29)

#### Global constraints

- Work against the shipped 0.85.28 state. Follow strict RED, GREEN, refactor
  TDD for each schema transition, lifecycle boundary, status field, and removed
  compatibility seam. No production behavior changes before its focused test
  fails for the expected reason.
- Make schema v13 and the 0.86.0 startup migration the durable cutover
  boundary. Every migration has idempotent `up` and `down`, takes an immediate
  SQLite writer before reading mutable shape, repairs partial same-version
  state, and leaves help and version side-effect free.
- Preserve the product boundary established by BR-12 through BR-17: one live
  TUI processes one receiver job at a time for its workspace; the database is
  queue, conversation, transcript, claim, answer, cleanup, and delivery
  authority; provider IO never reruns agent work.
- Remove receiver use of interactive `AgentController` input while preserving
  those semantic methods for ordinary user-driven panel input. Claude, Codex,
  and OpenCode must continue to launch receiver prompts only as initial launch
  data and complete through the same controller-owned lifecycle contract.
- Treat logs, status, errors, Debug output, tests, and migration diagnostics as
  privacy boundaries. They may contain counts, finite phase names, attempt
  ordinals, deadlines, and stable reason codes, but never prompts, answers,
  transcripts, recipients, senders, attachment paths, provider bodies,
  credentials, or serialized envelopes.
- Commit each independently reviewed SDD task with the required version bump.
  Require one fresh whole-branch review after all tasks and correct every
  load-bearing finding before integration.

#### Task 1: Replace the legacy notice bit with a cleanup-gated outbox state

Start with failing schema, migration, terminalization, cleanup, downgrade,
concurrency, corruption, and privacy tests.

The green implementation must:

- Add a durable cleanup-gated unavailable-notice state to the response outbox,
  so terminal recovery writes one immutable semantic response in its existing
  transaction instead of setting `pending_unavailable_notice` for a later
  in-memory decision.
- Promote that response to ordinary delivery only after exact recovery cleanup
  authority clears. A crash before or after cleanup acknowledgement must leave
  either the cleanup-gated response or the ready response, never neither and
  never both.
- Upgrade existing v12 rows transactionally: convert each legacy pending bit
  to one immutable outbox row, preserve cleanup fences, move cleanup-free rows
  to ready delivery, terminalize only unrenderable or unauthorized legacy
  state with a stable reason, and rebuild `receiver_jobs` without the pending
  bit or the two obsolete notice-claim columns.
- Update all recovery, terminalization, load, completion, fallback, repair, and
  cleanup queries to the v13 contract. Remove runtime reads and writes of the
  legacy columns only after the migration proves semantic equivalence.
- Downgrade v13 atomically to the exact v12 contract. Reconstruct a pending
  notice bit for cleanup-gated rows, retain already representable ready and
  terminal deliveries, prevent agent replay, restore indexes and constraints,
  and prove down/up idempotence under concurrent writers and partial schemas.

#### Task 2: Remove `jobs.sock` and cut server liveness to lease authority

Start with failing startup, partial-acquisition, control-protocol, route,
multi-workspace, stale-file, mixed-version, and migration tests.

The green implementation must:

- Remove `JobSocket`, `WorkspacePaths::job_socket`, the receiver runtime owner,
  builder acquisition stage, shutdown generic, lease registration field,
  server-side connect probe, route equality check, and every test or document
  that treats a local socket as receiver availability.
- Make the existing elected server lease the sole live-TUI route authority:
  workspace UUID, ingress ID, generation, lease ID, selected root, TUI PID,
  receiver intent, heartbeat, and expiry must still match at registration,
  final admission, revocation, status, and route lookup boundaries.
- Add an automatic cutover migration that removes only an exact stale
  owner-controlled Unix socket leaf. It must not follow or unlink a symlink,
  regular file, replacement, or a socket still serving an older live Brain.
  Downgrade is explicit and idempotent because a v12 binary recreates its own
  endpoint when it starts.
- Add a bounded protocol-version fence. A new TUI encountering an older live
  shared server must receive a clear restart diagnostic rather than silently
  restoring the legacy endpoint or corrupting lease state; once older TUIs
  close, election and registration proceed automatically.
- Preserve orderly and crashed final-lease server shutdown. The shared-server
  heartbeat watchdog is current lease infrastructure, not the removed BR-10
  warm-panel inactivity watchdog.

#### Task 3: Make the injection-free architecture a permanent structural rule

Start with failing structural mutations that restore each forbidden path, then
remove the final compatibility assertions and representation.

The green implementation must:

- Replace the transitional BR-18 `jobs.sock` assertion with final guards that
  reject any receiver-owned `JobSocket`, `InboundJob` `VecDeque` or channel
  consumer, warm-panel lease, main-panel takeover, screen/activity timeout,
  `AgentController::type_text`, `submit_now`, or queued interactive prompt.
- Keep legitimate transient effect holders narrow and explicitly non-
  authoritative: the one current receiver-run effect, attachment and provider
  worker result queues, and answer-controller cleanup queue may schedule local
  work, but every restart decision and terminal transition reloads exact
  database authority.
- Rename stale warm or remote-session vocabulary where it describes a fresh
  isolated run identity, and delete compatibility code, dead seams, and old
  BR-10 dispatch/completion tests that no longer prove current behavior.
- Preserve generic interactive controller input, the main brain panel, skill
  sessions, delivery executor queues, server lease watchdog, and test fixture
  queues. Structural analysis must distinguish those valid consumers from a
  receiver job queue instead of banning `VecDeque` or submit methods globally.
- Prove all three frontends use only initial launch prompts for fresh and
  native-resume receiver runs, and that no receiver path can borrow the main
  interactive controller or selected panel.

#### Task 4: Expose one redacted durable work snapshot in status and logs

Start with failing pure summary, formatter, read-only store, palette, CLI,
transition-log, malformed-state, and privacy tests.

The green implementation must:

- Add a content-free `ReceiverWorkSummary` derived in one read-only database
  snapshot. Define durable agent queue depth, oldest active phase, recovery
  attempt and limit, cleanup-gated response count, and delivery counts without
  decoding content-bearing inbound, transcript, answer, or envelope fields.
- Render the summary through semantic `Theme` tokens in `brain receiver`,
  `brain receiver status`, and the TUI receiver-status action. Missing or
  unavailable peer state must be honest rather than inventing zero work.
- Emit stable content-free lifecycle log records only after durable commits at
  ingress, claim, launch, acceptance/progress, recovery, answer readiness,
  cleanup promotion, delivery result, and terminal advancement. Queue depth,
  phase, recovery ordinal, delivery phase, and reason code are allowed; private
  identity and content are not.
- Keep status queries side-effect free beyond the normal pre-dispatch startup
  migration, keep `brain server logs` a faithful view of the same records, and
  verify themed human output plus deterministic plain output under
  `Theme::dark(false)`.

#### Task 5: Characterize preserved behavior and finish the cutover

Write final RED characterization before deleting any stale test seam. Cover
SMS and email routing, signature and actor authorization, destination
revocation, control commands, attachments, sync freshness, exact completion
push, task reload, immutable response rendering, provider retry, and FIFO
advancement.

The green implementation must:

- Exercise shutdown and App reconstruction at queued, claimed, launching,
  launched, accepted, processing, recovery, cleanup-gated response,
  answer-ready, delivering, retrying, acknowledged, failed, and done phases.
  Each nonterminal state must either resume safely or terminalize explicitly,
  and later jobs must never wait on an unowned process-local object.
- Prove all existing routing and security race fences remain in force after
  socket removal, including final admission, lease expiry, receiver disable,
  workspace isolation, capacity, deduplication, attachment staging, and exact
  completion attribution.
- Remove or rewrite obsolete tests around the former warm panel, injection,
  coarse inactivity, socket handoff, and local completion queue. Retain history
  in decisions only where it explains a current invariant.
- Audit and update architecture, features, integrations, data model, decisions,
  glossary, keybindings, config, testing, and the docs index where their stated
  behavior changes. Do not manufacture keybinding or config changes when the
  cutover has none.
- Finish with focused release suites for v13 up/down/repair, cleanup-gated
  notices, control registration and route authority, all-frontends launch,
  status/log privacy, and restart recovery; then run
  `cargo test --release -- --test-threads=1`,
  `cargo clippy --release --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, and structural checks for unsafe code,
  injection, process-local receiver authority, privacy, module size, version,
  documentation, and diff cleanliness.

### Self-review (2026-08-29)

- The old inbound socket queue, warm-panel executor, screen sampler, and BR-10
  inactivity policy are already gone. The plan does not rebuild or remove them
  twice; it removes the remaining lifetime-only `jobs.sock` representation and
  converts transitional absence tests into permanent cutover guards.
- `jobs.sock` is not current queue authority, but it still affects startup,
  lease registration, route equality, tests, paths, and mixed-version behavior.
  Treating its removal as a field deletion would weaken availability or strand
  a new TUI behind an older server, so Task 2 includes lease-only authority,
  stale-leaf migration, and an explicit protocol-version fence.
- The server lifecycle watchdog is necessary for expired crashed leases and
  shared-process shutdown. It is distinct from the removed warm-panel activity
  watchdog, so the plan preserves it and narrows obsolete-watchdog cleanup to
  receiver execution heuristics.
- Not every `VecDeque` is legacy receiver authority. Delivery workers,
  attachment workers, controller cleanup, test clocks, and fixtures use bounded
  queues legitimately. The structural rule targets `InboundJob` storage and
  receiver decisions derived from process-local queue state.
- The current recovery path still writes `pending_unavailable_notice`, so
  merely dropping its columns would lose terminal responses. A cleanup-gated
  outbox state gives the migration a durable semantic target and lets down
  reconstruct v12 without making failed agent work replayable.
- A cleanup-gated response cannot become provider-ready until exact native and
  artifact cleanup clears. Task 1 therefore couples promotion to the existing
  cleanup acknowledgement transaction rather than to an App-local follow-up.
- Interactive `type_text` and `submit_now` remain required for normal user
  input. The no-injection proof is scoped to receiver ownership and verifies
  the concrete controller type, preventing both false positives and aliases
  that bypass a string search.
- Status must not deserialize private rows just to count them. Task 4 requires
  aggregate SQL over finite states and reason codes, with an honest unavailable
  result for unreadable peer workspaces.
- Restart coverage includes delivery and cleanup-gated terminal notices, not
  only agent execution. That closes the last path where a completed or failed
  agent turn could otherwise depend on a surviving App object.
- The scope grew from an estimated 8 to 13 because current-code review exposed
  a reversible v13 schema cutover, shared-server protocol transition, and
  durable status/log contract in addition to code deletion. Splitting these
  into five reviewed commits keeps each RED/GREEN boundary independently
  auditable.

### Log

- 2026-08-23 created from PROJ-1 planning.
- 2026-08-23 removed completed BR-13 from the cutover prerequisites; BR-14
  through BR-17 still block the final cutover.
- 2026-08-29 unblocked after BR-14 through BR-17 shipped. Current-code review
  replaced the stale removal-only outline with a five-task plan for schema-v13
  notice cutover, lease-only server authority, permanent no-injection guards,
  redacted durable status and logs, and full restart characterization.
- 2026-08-29 completed in Brain 0.86.13. Schema v13 now provides a reversible
  cleanup-gated outbox cutover; server routing uses lease-only authority with a
  protocol fence; structural guards prohibit receiver injection and
  process-local queue authority; receiver status and lifecycle logs are
  content-free; and fresh-App recovery is characterized across all 14 durable
  lifecycle phases for Claude, Codex, and OpenCode.
- 2026-08-29 final whole-branch review found no critical, important, or minor
  findings after correction. Release tests, strict Clippy, formatting, privacy,
  structural, version, and documentation gates passed. Merged to `main` in
  `074b14a` and installed as Brain 0.86.13.
