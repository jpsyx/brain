# BR-16 Task 2 Report

## Status

DONE

## Summary

Task 2 turns BR-15's one-time progressing boundary into repeated, bounded,
content-free progress pulses without adding the recurring reconciler or recovery
launch effects reserved for later BR-16 tasks. The change:

- extends the exact schema-v1 snapshot from ten to eleven fields with
  `latest_progress_at_unix_ms`, while retaining the first accepted and first
  progressing timestamps;
- makes second and later exact `PostToolUse` events advance the revision and
  latest progress time, while rejecting immediate duplicates and non-advancing
  producer time;
- keeps Claude, Codex, and OpenCode behind `AgentController`, with equivalent
  pulse-only normalized results and durable cursor behavior;
- makes OpenCode use its exact tool call ID, revoke receiver eligibility on a
  later root user turn, and reject delayed marker parts from an older user
  message;
- applies pulses in one immediate exact-owner, token, instance, native-session,
  live-registration, and monotonic-revision transaction;
- renews the five-minute progress deadline from fresh local authorization time
  through `ReceiverLifecycleDeadlines::after_progress`, clamped to the original
  immutable absolute deadline, while producer time remains evidence only;
- preserves owner-only atomic snapshot replacement, the 4096-byte bound,
  content privacy, self-healing installed artifacts, and exact completion at
  revision saturation; and
- updates the architecture, data model, behavior, integration, decision, and
  testing documentation for the new neutral progress-pulse contract.

The crate version moved from 0.84.2 to 0.84.3 in the Task 2 product commit.

## RED evidence

Every production slice began with a focused failing test. The following
failures were observed before its corresponding production change.

### Repeated bridge pulse

Command:

```text
cargo test --release --test receiver_observation_bridge later_progress_pulses -- --nocapture
```

Observed failure excerpt:

```text
assertion `left == right` failed
  left: Null
 right: 1100
```

The existing bridge retained only the first progressing boundary and had no
latest-progress field for a later exact tool event.

### OpenCode exact tool identity and later pulse

Command:

```text
cargo test --release --test opencode_plugin plugin_records_receiver_acceptance_and_progress_from_incremental_events_only -- --exact --nocapture
```

Observed failure excerpt:

```text
assertion `left == right` failed
  left: "assistant-1"
 right: "turn-1"
```

OpenCode used the containing message ID instead of the native call ID and did
not publish the later pulse.

### Fixed-size privacy contract

Command:

```text
cargo test --release --test receiver_observation_privacy submit_tool_and_stop_producers_keep_private_content_out_of_observations_and_output -- --exact --nocapture
```

Observed failure excerpt:

```text
assertion `left == right` failed
  left: 10
 right: 11
```

The snapshot had not yet gained the one content-free latest-progress field.

### Installer self-healing

Command:

```text
cargo test --release --test hook_integration self_heals_repeated_progress -- --nocapture
```

Observed failure excerpt:

```text
assertion failed: second_progress.status.success()
```

A stale deployed bridge rejected the second required progress write, proving
the managed bridge and plugin needed reconciliation coverage.

### Neutral cursor and result

Command:

```text
cargo test --release agent::observation::tests::newer_revision_can_return_a_progress_pulse_without_a_new_lifecycle_phase -- --exact --nocapture
```

Observed failure excerpt:

```text
error[E0599]: no method named `progress_pulse` found
error[E0061]: this function takes 4 arguments but 5 arguments were supplied
error[E0560]: struct `ReceiverObservationSet` has no field named
`latest_progress_at_unix_ms`
```

The frontend-neutral API could return lifecycle boundaries only.

### Delayed prior OpenCode marker

The first green implementation revoked eligibility on a newer root user turn.
Self-review then added a smaller red for an old marker part delivered after that
turn.

Command:

```text
cargo test --release --test opencode_plugin plugin_records_receiver_acceptance_and_progress_from_incremental_events_only -- --exact --nocapture
```

Observed failure excerpt:

```text
AssertionError [ERR_ASSERTION]: an unrelated user turn must revoke progress eligibility
  revision: 4
  turn_id: 'turn-unrelated'
```

The old message ID remained in a bounded history map and could re-authorize the
session until the plugin also required the current root user message ID.

## GREEN and refactor evidence

### Focused Task 2 tests

Commands and results:

```text
cargo test --release agent::observation::tests
test result: ok. 13 passed; 0 failed

cargo test --release --test receiver_observation_bridge
test result: ok. 15 passed; 0 failed

cargo test --release --test opencode_plugin
test result: ok. 7 passed; 0 failed

cargo test --release --test receiver_observation_privacy
test result: ok. 12 passed; 0 failed

cargo test --release state::receiver::tests
test result: ok. 85 passed; 0 failed

cargo test --release --test hook_integration
test result: ok. 23 passed; 0 failed

cargo test --release tui::app_brain::tests::receiver_durable_observation
test result: ok. 83 passed; 0 failed

cargo test --release --no-run
Finished `release` profile; exit code 0
```

Additional exact tests passed for pulse-only results through all three adapters,
the exact durable deadline transaction, all-three-frontend App persistence,
registry encapsulation, installer reconciliation, revision saturation, duplicate
and reordered producer events, and the delayed OpenCode marker.

### Complete suite coverage

Command:

```text
cargo test --release -- --test-threads=1
```

The library target passed all 2254 tests. All Task 2 integration binaries also
passed. Two long serial attempts each encountered one different unrelated
short-deadline server-fixture timeout later in the run:

```text
receiver_workspace_isolation: value was not produced within 3s
server_lifecycle: brain server did not come up within 2s
```

Systematic isolation showed both were transient startup timing failures. Each
exact test and each complete integration binary passed unchanged:

```text
cargo test --release --test receiver_workspace_isolation \
  persisted_disable::persisted_disable_rejects_and_enqueues_nothing_before_control_refresh \
  -- --exact --test-threads=1 --nocapture
test result: ok. 1 passed; 0 failed

cargo test --release --test receiver_workspace_isolation -- --test-threads=1
test result: ok. 24 passed; 0 failed

cargo test --release --test server_lifecycle \
  recovery::cleanup_failure_after_publication_keeps_waiter_for_removed_token_recovery \
  -- --exact --test-threads=1 --nocapture
test result: ok. 1 passed; 0 failed

cargo test --release --test server_lifecycle -- --test-threads=1
test result: ok. 25 passed; 0 failed
```

Every integration target after the second abort point was then run together and
passed with exit code 0. Combined with the green targets before that point, this
covers every release test target without changing unrelated timeout policy.

### Lint, formatting, and patch hygiene

Commands and results:

```text
cargo clippy --release --all-targets -- -D warnings
Finished `release` profile; exit code 0

cargo fmt --all -- --check
exit code 0

git diff --check
exit code 0
```

Clippy prompted two mechanical refactors: `AgentObservationResult::has_updates`
is not `const`, preserving the crate's Rust 1.85 MSRV, and one test uses lazy
`Option::and_then` evaluation.

## Commits

- `feat(receiver): renew progress with exact pulses` contains the 0.84.3 Task 2
  product, tests, docs, and this report. Its final hash is supplied in the task
  handoff because embedding the hash in the commit's own report would change
  that hash.

## Files changed

- Release metadata: `Cargo.toml`, `Cargo.lock`.
- Product documentation: `docs/architecture.md`, `docs/data-model.md`,
  `docs/decisions.md`, `docs/features.md`, `docs/integrations.md`,
  `docs/testing.md`.
- Managed producers: `scripts/opencode_brain_plugin.js`,
  `scripts/receiver_observation_bridge.py`.
- Neutral agent boundary: `src/agent/mod.rs`, `src/agent/observation.rs`,
  `src/agent/observation/snapshot.rs`.
- Durable receiver state: `src/state/receiver/model.rs`,
  `src/state/receiver/store/completion.rs`,
  `src/state/receiver/store/observation.rs`.
- Coordinator conversion and effects: `src/tui/state/services.rs`,
  `src/tui/app_brain/receiver/active.rs`.
- Rust tests and fixtures: `src/agent/adapter_tests/contract.rs`,
  `src/agent/observation/file_tests.rs`, `src/agent/observation/tests.rs`,
  `src/state/receiver/tests/binding.rs`, `src/state/receiver/tests/launch.rs`,
  `src/tui/app_brain/tests/opencode_receiver.rs`,
  `src/tui/app_brain/tests/receiver_durable_cleanup.rs`,
  `src/tui/app_brain/tests/receiver_durable_observation.rs`,
  `src/tui/app_brain/tests/receiver_durable_observation_composed.rs`,
  `src/tui/app_brain/tests/receiver_durable_observation_continuity.rs`,
  `src/tui/app_brain/tests/receiver_durable_observation_replacement.rs`,
  `src/tui/app_brain/tests/receiver_durable_producer_saturation.rs`,
  `tests/agent_registry_boundary.rs`, `tests/hook_integration/installer.rs`,
  `tests/receiver_observation_bridge.rs`,
  `tests/receiver_observation_bridge/producer_boundaries.rs`,
  `tests/receiver_observation_privacy.rs`.
- OpenCode harness: `tests/fixtures/opencode/plugin_harness.js`.
- Delivery record: `.superpowers/sdd/BR-16-recover-stalled-receiver-jobs/task-2-report.md`.

## Self-review

- Scope remains Task 2 only. No recurring stalled-job reconciler, recovery
  launch orchestration, unavailable notice, or App recovery effect was added.
- The coordinator sees only neutral boundaries and `AgentProgressPulse`; it does
  not name provider event fields or snapshot grammar.
- The current-attempt durable cursor uses latest progress only when that attempt
  has progressing evidence, so Task 1 recovery reset cannot inherit lifetime
  liveness state.
- First accepted and first progressing evidence remain immutable. Completion
  merges and clamps against the latest pulse, including artifact precedence and
  saturated no-mutation Stop paths.
- Pulse authorization is local transaction time. A future producer timestamp
  cannot extend runtime, and claim expiry remains only the writer fence.
- The snapshot remains exact-schema, content-free, owner-only, atomically
  replaced, and bounded to 4096 bytes. Privacy and self-healing tests cover the
  changed managed artifacts.
- The delayed-marker self-review red closed the only event-ordering gap found
  after the initial green implementation.

## Concerns

- The full serial release command exposed two different pre-existing,
  load-sensitive server fixture startup deadlines. Both exact tests and their
  complete binaries passed unchanged, and every release target received green
  coverage. No unrelated timeout change was made in Task 2.
- No Task 2 product concern remains.

## Fix round 1

### Status

DONE

### Summary

Review round 1 closes the two Task 2 correctness gaps without entering Task 3
or Task 4 scope:

- Claude progress is authorized only when each `PostToolUse` carries the exact
  `prompt_id` accepted from the receiver marker turn. Codex applies the same
  rule with its exact `turn_id`; both use `tool_use_id` as the distinct pulse
  identity.
- A later non-marker root prompt in the same Claude or Codex session clears the
  content-free accepted-turn authority retained in the existing owner-only
  producer lock. Delayed receiver callbacks and tools from the unrelated turn
  cannot publish a pulse.
- OpenCode binds each assistant message to its exact parent root user message.
  A tool callback is eligible only when its assistant message belongs to the
  exact accepted receiver user message. A delayed callback from an older turn
  is rejected even when it arrives after receiver acceptance, and a later root
  user message still revokes eligibility.
- `AgentObservationResult::snapshot_revision` is removed. Raw producer revision
  remains inside `AgentObservationCursor` and crosses into durable state only
  in `ReceiverObservationSet::from_agent_observation`, the agent-to-state seam.
  `src/tui/state/services.rs` now handles only the neutral result, completion
  decision, and converted durable set.
- The structural observation guard now scans the actual services conversion
  path, rejects both direct raw-revision access forms, and retains a neutral
  conversion control case.
- Managed bridge/plugin installation, privacy, completion, and all-frontend
  producer matrix fixtures now exercise exact accepted-turn correlation.

The crate version moved from 0.84.3 to 0.84.4 for this fix commit.

### RED evidence

#### Delayed prior Claude/Codex tool callback

Command:

```text
cargo test --release --test receiver_observation_bridge claude_and_codex_reject_delayed_tool_events_from_a_prior_turn_after_acceptance -- --exact --nocapture
```

Observed failure excerpt:

```text
assertion `left == right` failed: claude accepted a delayed tool event from a prior turn
  left: ... "phase": "progressing" ... "revision": 2 ...
 right: ... "phase": "accepted" ... "revision": 1 ...
test result: FAILED. 0 passed; 1 failed
```

The shared bridge checked token, instance, and native session but did not bind
the tool event to the exact accepted receiver turn.

#### Later non-marker Claude/Codex prompt

Command:

```text
cargo test --release --test receiver_observation_bridge claude_and_codex_revoke_progress_after_a_later_nonmarker_root_prompt -- --exact --nocapture
```

Observed failure excerpt:

```text
assertion `left == right` failed: claude retained progress authority after an unrelated prompt
  left: ... "revision": 4 ... "turn_id": "unrelated-tool" ...
 right: ... "revision": 2 ... "turn_id": "receiver-tool-1" ...
test result: FAILED. 0 passed; 1 failed
```

A non-marker `UserPromptSubmit` returned before changing any producer
authorization, so later same-session tool events could renew the receiver.

#### Delayed prior OpenCode assistant callback

Command:

```text
cargo test --release --test opencode_plugin plugin_records_receiver_acceptance_and_progress_from_incremental_events_only -- --exact --nocapture
```

Observed failure excerpt:

```text
AssertionError [ERR_ASSERTION]: a delayed tool callback from a prior unrelated turn must not progress
+   phase: 'progressing'
+   revision: 2
-   phase: 'accepted'
-   revision: 1
test result: FAILED. 0 passed; 1 failed
```

The plugin retained a session-keyed boolean and did not prove that the callback
message belonged to the accepted root user message.

#### Raw snapshot revision boundary leak

Command:

```text
cargo test --release --test agent_registry_boundary receiver_observation_coordination_cannot_name_provider_or_snapshot_grammar -- --exact --nocapture
```

Observed failure excerpt:

```text
src/tui/state/services.rs bypasses AgentController observation ownership
  left: Some("snapshot_revision")
 right: None
test result: FAILED. 0 passed; 1 failed
```

The expanded structural guard proved that the real conversion path could name
and extract parser revision grammar.

### GREEN and refactor evidence

Focused review fixes:

```text
cargo test --release --test receiver_observation_bridge claude_and_codex_ -- --nocapture
test result: ok. 2 passed; 0 failed

cargo test --release --test opencode_plugin plugin_records_receiver_acceptance_and_progress_from_incremental_events_only -- --exact --nocapture
test result: ok. 1 passed; 0 failed

cargo test --release --test agent_registry_boundary receiver_observation_ -- --nocapture
test result: ok. 2 passed; 0 failed

cargo test --release receiver_durable_producer_matrix -- --nocapture
test result: ok. 1 passed; 0 failed
```

Complete changed integration surfaces:

```text
cargo test --release --test receiver_observation_bridge --test opencode_plugin --test agent_registry_boundary --test receiver_observation_privacy --test hook_integration --test stop_hook_actor -- --test-threads=1
agent_registry_boundary: 6 passed
hook_integration: 23 passed
opencode_plugin: 7 passed
receiver_observation_bridge: 17 passed
receiver_observation_privacy: 12 passed
stop_hook_actor: 11 passed
```

Neutral observation and lint gates:

```text
cargo test --release agent::observation::tests -- --nocapture
test result: ok. 13 passed; 0 failed

cargo clippy --release --all-targets -- -D warnings
Finished `release` profile; exit code 0

cargo fmt --all
python3 -c 'import ast, pathlib; ast.parse(pathlib.Path("scripts/receiver_observation_bridge.py").read_text())'
node --check scripts/opencode_brain_plugin.js
git diff --check
exit code 0
```

The first expanded bridge, installer, privacy, and App matrix runs found legacy
fixtures that omitted `BRAIN_AGENT_KIND` or the new content-free correlation
fields. Systematic inspection confirmed every failure occurred before snapshot
creation. The fixtures were updated to the documented Claude, Codex, or
OpenCode payload contract; production validation remained fail-closed.

### Complete uninterrupted serial suite

Command:

```text
cargo test --release -- --test-threads=1
```

Result:

```text
Finished `release` profile
library: 2254 passed; 0 failed
all integration targets: passed
doc tests: 0 passed; 0 failed
process exit code: 0
```

This was one uninterrupted invocation. No failure, retry, rerun, timeout-policy
change, or partial-target substitution occurred.

### Commit

- `fix(receiver): bind progress to accepted turn` contains the 0.84.4 product,
  tests, docs, and this round-1 report. The final hash is supplied in the task
  handoff because embedding a commit's own hash in its contents would change
  that hash.

### Files changed

- Release metadata: `Cargo.toml`, `Cargo.lock`.
- Product documentation: `docs/architecture.md`, `docs/data-model.md`,
  `docs/decisions.md`, `docs/features.md`, `docs/integrations.md`,
  `docs/testing.md`.
- Managed producers: `scripts/opencode_brain_plugin.js`,
  `scripts/receiver_observation_bridge.py`.
- Opaque agent-to-state seam: `src/agent/observation.rs`,
  `src/agent/observation/tests.rs`, `src/state/receiver/model.rs`,
  `src/tui/state/services.rs`.
- Producer and boundary coverage:
  `src/tui/app_brain/tests/receiver_durable_producer_matrix.rs`,
  `tests/agent_registry_boundary.rs`,
  `tests/fixtures/opencode/plugin_harness.js`,
  `tests/hook_integration/installer.rs`,
  `tests/receiver_observation_bridge.rs`,
  `tests/receiver_observation_bridge/fixture_support.rs`,
  `tests/receiver_observation_bridge/producer_boundaries.rs`,
  `tests/receiver_observation_privacy.rs`,
  `tests/stop_hook_actor/contracts.rs`.
- Delivery record:
  `.superpowers/sdd/BR-16-recover-stalled-receiver-jobs/task-2-report.md`.

### Self-review

- The exact producer contract uses documented stable identifiers: Claude
  `prompt_id`, Codex `turn_id`, and both frontends' `tool_use_id`. Missing or
  malformed identifiers fail closed.
- OpenCode requires a root assistant `message.updated` event whose `parentID`
  names the accepted root user message before its tool callback can progress.
  Root-session, child-session, current-message, and bounded-map exclusions are
  preserved.
- The existing owner-only lock now carries a fixed five-field, content-free,
  bounded authorization record. Snapshot schema-v1 remains exactly eleven
  fields, owner-only, content-free, and at most 4096 bytes.
- Non-marker root prompts clear only exact token, instance, session authority.
  They do not mutate accepted/progress/completion evidence or increment the
  snapshot revision.
- Raw revision no longer escapes through `AgentObservationResult`. The only
  extraction is inside the state model conversion seam; the services and
  receiver coordinator paths are structurally guarded against both old and new
  raw accessor names.
- Claim expiry remains writer fencing only. No recurring reconciler, recovery
  launch, unavailable notice, or App recovery effect was added.
- Managed bridge and plugin contents remain registry-declared, installer
  reconciled, startup self-healed, and doctor checked through their existing
  exact-file contracts.

### Concerns

- Claude's exact `prompt_id` hook field requires Claude Code 2.1.196 or later.
  Older Claude versions now fail closed for receiver acceptance/progress rather
  than accepting session-only evidence. No Task 2 correctness concern remains.
