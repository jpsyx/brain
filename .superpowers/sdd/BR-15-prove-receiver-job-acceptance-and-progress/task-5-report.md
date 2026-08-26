# BR-15 Task 5 Report: Parity, Privacy, and Restart Hardening

## Status

Complete. BR-15 now has an explicit Claude, Codex, and OpenCode producer matrix,
conservative snapshot replacement and revision-saturation coverage, fresh-App
restart characterization, a repository-level observation privacy guard, and
aligned lifecycle documentation. The compatible hardening release is `0.83.3`.
BR-16 stalled-run recovery, BR-17 durable answer and delivery recovery, and
BR-18 legacy representation removal remain excluded.

## Inventory and scope decisions

The Task 1 through Task 4 inventory found existing coverage for schema-v9 up,
down, damaged-state reconciliation, token-collision retry bounds, immutable job
IDs and inbound bytes, exact token rejection in producer, reader, coordinator,
and completion paths, concurrent producer monotonicity, durable cursor rebuild,
session rotation, isolated tab and focus behavior, attachments, controls,
shutdown, exact artifact completion, FIFO release, sync gates, and help/version
side-effect freedom.

That inventory exposed two genuine behavior gaps:

1. The Python producer could increment revision past `i64::MAX`, even though
   SQLite cannot represent the result.
2. Expired `accepted` and `processing` rows were lease-replaced by claim
   polling. A fresh process did not relaunch them, but it still mutated their
   owner and lease before BR-16 had supplied stalled-run recovery policy.

The remaining requested work was characterization or privacy enforcement.
No production change was manufactured to create a RED, and no Task 1 through
Task 4 deterministic test seam was removed.

## Red and green evidence

1. **Producer revision saturation**
   - RED: `cargo test --release --test receiver_observation_bridge revision_saturation_preserves_the_last_valid_snapshot_for_later_events -- --exact --nocapture` failed because a revision-`i64::MAX` accepted snapshot was replaced by revision `9223372036854775808` after progress.
   - GREEN: the same focused test passed after the bridge capped producer
     revisions at SQLite's signed maximum and preserved the last valid snapshot
     for later progress or completion events.

2. **Fresh-process no-reclaim boundary**
   - RED: `cargo test --release --lib state::receiver::tests::recovery::expired_observed_lifecycles_remain_unchanged_until_stalled_recovery_exists -- --exact --nocapture` failed because an expired accepted row was reassigned to the new process.
   - GREEN: the same focused test passed after claim selection stopped at
     expired `launched`, `accepted`, and `processing` rows without changing any
     durable field. The recovery module passed 9 tests after its earlier broad
     reclaim characterization was narrowed to eligible pre-acceptance and
     delivery phases.

3. **Three-frontend normalized producer matrix**
   - The first behavioral run found a test-fixture assumption: the duplicate
     OpenCode producer invocation already had a valid prior snapshot, so the
     reordered-progress assertion could not require the path to be absent. The
     harness was corrected to compare the prior bytes when evidence already
     exists. This was not a production RED.
   - GREEN: `normalized_producers_drive_one_controller_and_coordinator_lifecycle_matrix` passed. It drives the real Python hook bridge for Claude and Codex and the real incremental OpenCode plugin, then crosses the shared
     `AgentController` and durable App coordinator. All three prove progress
     before acceptance is a no-op, delayed exact acceptance and later progress
     advance to revision 2, duplicate submit/tool/completion delivery is
     idempotent, normal completion reaches revision 3 once, and completion-first
     reaches revision 1 with null intermediate timestamps.

4. **Observation-file replacement**
   - Initial construction found only test-helper compile errors, corrected
     before the first behavioral run. No production RED existed because the
     strict reader already enforced the requested behavior.
   - GREEN: `replaced_observation_files_never_advance_the_durable_job` passed for
     symlink, mode `0644`, wrong token, truncation, lower revision, and revision
     greater than `i64::MAX`. Each case preserves the complete durable row, the
     active receiver tab, and controller ownership.

5. **Fresh-App restart characterization**
   - Initial construction attempted to inspect a sibling test module's private
     database connection and failed to compile. The fixture was corrected to
     use an independent SQLite connection before its first behavioral run.
   - GREEN: `fresh_app_preserves_expired_launched_and_observed_runs_without_replay` passed. A newly constructed App preserves every token, instance,
     session, revision, timestamp, owner, and lease field for expired
     `launched`, `accepted`, and `processing` rows, creates no replacement tab,
     performs no controller shutdown, and does not replay a prompt.

6. **Actual bounded launch marker**
   - Existing launch budgeting already satisfied the behavior, so no new
     production RED was created.
   - GREEN: the actual Fresh and Resume command matrix for Claude, Codex, and
     OpenCode now asserts the exact job marker is the final prompt line, occurs
     once in the raw prompt and once in the completed command, stays within the
     47 KiB raw-prompt and 96 KiB command limits, and retains the documented
     current-message and attachment omission markers.

7. **Current-main characterization correction**
   - After the no-reclaim fix, the focused durable App subset found one older
     expectation still assuming an expired processing job was temporarily
     claimed and its tentative controller shut down.
   - GREEN: the expectation now requires zero shutdowns because no replacement
   controller is constructed. The exact regression and the complete 76-test
   durable App subset passed.

8. **Opaque token Debug redaction**
   - RED: `cargo test --release --test receiver_observation_privacy observation_diagnostics_and_debug_formatting_expose_no_private_fields -- --exact --nocapture` rendered the exact UUID as `ReceiverJobToken(11111111-1111-4111-8111-111111111111)`.
   - GREEN: the same focused test and the complete 3-test privacy suite passed
     after `ReceiverJobToken` received a fixed redacted Debug representation.
     Derived launch, observation, completion, and durable-job formatting can no
     longer reveal the token value.

## Implementation summary

- Bounded the normalized producer revision at `i64::MAX` and retained the last
  representable snapshot after saturation.
- Made durable claim selection stop conservatively at expired `launched`,
  `accepted`, and `processing` work, preserving all correlation and FIFO facts
  for BR-16.
- Added one all-frontend producer-to-controller-to-App lifecycle matrix using
  only synthetic opaque values and supported normalized producer surfaces.
- Added full snapshot-replacement and fresh-App restart characterization.
- Strengthened the actual command-budget tests for exact terminal marker
  placement and uniqueness.
- Added a repository-level privacy guard covering 20 production, fixture, and
  test sources, redacted Debug/Error rendering, diagnostic field exclusions,
  and a submitted canary that must not enter the ten-field snapshot.
- Redacted `ReceiverJobToken` at its Debug boundary so every derived containing
  type inherits token-value redaction.
- Updated architecture, features, integrations, data model, decisions, and
  testing documentation for saturation, no-reclaim restart behavior, privacy,
  and the BR-16/17/18 boundaries.
- Bumped `Cargo.toml` and `Cargo.lock` from `0.83.2` to `0.83.3`.

## Verification

Focused release verification:

- Receiver durable state: 63 passed, 0 failed.
- Python observation bridge: 8 passed, 0 failed.
- Observation privacy guard: 3 passed, 0 failed.
- OpenCode plugin: 7 passed, 0 failed.
- Lifecycle hook integration: 22 passed, 0 failed.
- Agent observation reader and cursor: 21 passed, 0 failed.
- All-frontend controller observation contract: 1 passed, 0 failed.
- Durable App receiver behavior: 76 passed, 0 failed.
- Schema and startup migrations: 14 passed, 0 failed.
- Completion and stop-hook continuity: 10 passed, 0 failed.
- Actual Fresh/Resume terminal marker: 1 passed, 0 failed.

Repository-wide verification:

- `cargo test --release -- --test-threads=1`: 3,007 tests inventoried; all
  library, integration, and documentation targets passed. The library target
  passed 2,222 tests, 0 failed.
- `cargo clippy --release --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `cargo test --release --test module_structure`: 1 passed, 0 failed.
- `cargo test --release --test agent_registry_boundary`: 6 passed, 0 failed.
- `cargo test --release --test tui_receiver_runtime_architecture`: 4 passed,
  0 failed.
- `cargo test --release --test tui_receiver_dispatch_architecture`: 168 passed,
  0 failed.
- `cargo test --release --test tui_dependencies_architecture`: 8 passed,
  0 failed.
- `cargo test --release --test tui_state_aggregates_architecture`: 5 passed,
  0 failed.
- `git diff --check`: passed.
- Added-line and new-file scans found no fixed sleeps, `unsafe`, em dashes,
  local personal paths, or private hosts. The privacy guard's forbidden-pattern
  table intentionally contains the patterns it rejects and is not fixture data.
- The changed-file audit found no `docs/product-manager/**` or sealed review or
  rereview artifact changes.

The first complete single-thread suite passed all 2,222 library tests, then one
unchanged `receiver_workspace_isolation` deadline test reported that its value
was not produced. Systematic reproduction passed the exact test 1 of 1 and the
entire integration target 24 of 24. No code was changed for the transient. The
required complete single-thread rerun then passed every one of the 3,007 listed
tests, including that integration target.

After the late token-Debug RED/GREEN, the first post-fix full run again passed
all 2,222 library tests, then an unchanged `multi_workspace_acceptance` server
fixture missed its startup deadline. The exact test passed 1 of 1 and its whole
integration target passed 4 of 4 immediately, with no code change. A final
complete single-thread rerun against the redacted token type passed all 3,007
listed tests, including both timing-sensitive workspace targets.

## Privacy review

The normalized snapshot remains exactly ten fields and at most 4096 bytes. It
contains only schema version, monotonic revision, lifecycle phase, opaque token,
opaque instance, bounded session and turn IDs, and three lifecycle timestamps.
The new canary test proves submitted prompt text is absent from the serialized
snapshot.

The opaque job token, observation request, and observation-set Debug output are
fully redacted.
Observation errors reveal only stable categories. Coordinator diagnostics do
not name or format token, prompt, body, message, response, sender, recipient,
credential, snapshot, transcript, or path fields. The producer matrix, plugin
harness, restart fixtures, and replacement fixtures contain only synthetic
opaque identities and generic labels; no live account, private transcript,
personal filesystem path, credential, email address, or private host was used.

## Changed files

- `Cargo.toml`
- `Cargo.lock`
- `scripts/receiver_observation_bridge.py`
- `src/state/receiver/store/claim/next.rs`
- `src/state/receiver/model.rs`
- `src/state/receiver/tests/recovery.rs`
- `src/tui/app_brain/tests/mod.rs`
- `src/tui/app_brain/tests/receiver_durable_launch.rs`
- `src/tui/app_brain/tests/receiver_durable_observation_replacement.rs`
- `src/tui/app_brain/tests/receiver_durable_process_restart.rs`
- `src/tui/app_brain/tests/receiver_durable_producer_matrix.rs`
- `src/tui/receiver/planning_tests.rs`
- `tests/fixtures/opencode/plugin_harness.js`
- `tests/receiver_observation_bridge.rs`
- `tests/receiver_observation_privacy.rs`
- `docs/architecture.md`
- `docs/features.md`
- `docs/integrations.md`
- `docs/data-model.md`
- `docs/decisions.md`
- `docs/testing.md`
- `.superpowers/sdd/BR-15-prove-receiver-job-acceptance-and-progress/task-5-report.md`

## Concerns

No unresolved behavioral or privacy concern remains. The two transient
unchanged process-fixture deadlines are recorded above; focused reproduction,
their whole integration targets, and the authoritative final full rerun all
passed.

## Review fix round (2026-08-25)

This section supersedes the pre-review parity, replacement, numeric-boundary,
and privacy claims above. The sealed review found one critical and three
important gaps; all four are closed in version 0.83.4.

### RED and GREEN evidence

- Replacement laundering RED: a same-scope accepted job followed by a real
  `PostToolUse` event replaced an untrusted symlink snapshot with a new
  revision-2 `processing` file. GREEN: the Python bridge now opens an existing
  snapshot once with no-follow and nonblocking flags where available, verifies
  the opened descriptor before and after one bounded read, and accepts only a
  regular owner-only file no larger than 4096 bytes with the exact strict
  ten-field schema-v1 and trusted token, instance, and session lineage. Invalid
  symlink, permissive, malformed, truncated, wrong-identity, and ambiguous
  snapshots are preserved. In every focused case the durable row and active
  tab remain unchanged and the App emits only the expected stable,
  content-free diagnostic category.
- Final self-review found that Python's default JSON decoder collapsed
  duplicate object fields. The focused duplicate-field RED advanced the
  accepted snapshot to revision 2 even though the strict Rust reader rejected
  the prior bytes. GREEN: strict object decoding now rejects every duplicate
  field before schema validation, preserves the exact prior bytes, and emits
  the stable `malformed-snapshot` category without changing the durable row or
  active tab.
- Real-producer matrix RED: exact completion artifacts could persist the later
  App poll time instead of the actual lifecycle producer boundary, a 53 ms
  discrepancy in the Claude row. GREEN: when the same poll has an exact
  `completed` lifecycle boundary, artifact consumption persists that producer
  timestamp. The matrix now drives `launched`, `accepted`, `processing`, and
  `done` separately for Claude, Codex, and OpenCode. Claude and Codex terminate
  through `agent_session_stop_hook.py`; OpenCode terminates through its real
  `session.idle` plugin route into the stop hook. Normal and completion-first
  orderings, duplicate idempotence, and exact non-invented timestamps pass for
  all three frontends.
- Exact maximum revision characterization crossed the strict producer,
  reader/parser, `AgentController`, App conversion, SQLite persistence,
  durable-cursor reconstruction, and a subsequent no-newer-event poll on its
  first run. No production gap or manufactured RED was found. Revision
  `i64::MAX` remains exact and nonnegative, and its phase and timestamps remain
  intact after a saturated later producer event.
- The privacy runtime canaries passed on their first behavioral run, so no
  production privacy RED was manufactured. The structural guard initially
  reported two deliberately generic fixture-path false positives; its path
  heuristic was narrowed to the actual private-path pattern and then passed.
  Discovery is now recursive and semantic, has an explicit minimum breadth,
  and requires the stop hook, bridge, reader, completion producer/route/store,
  App controller/runtime/diagnostics, OpenCode harness, and all Task 5
  observation surfaces. Runtime canaries cross submit, tool, stop/completion,
  OpenCode plugin, Debug, error, and diagnostic paths. Private prompt, body,
  response, recipient, and credential values never enter snapshots or
  diagnostics; tokens remain absent from Debug and diagnostics while trusted
  observation and completion artifacts retain their intentional opaque
  identity fields.
- The stricter canonical-instance contract exposed synthetic non-UUID fixture
  identities in the bridge, OpenCode, and stop-hook tests. These were
  fixture-only failures. The fixtures now use stable canonical UUIDs, and the
  corresponding targets pass 8 of 8, 7 of 7, and 10 of 10.
- Strict Clippy found a test-only diagnostic capture method taking an
  unnecessary mutable receiver. The test seam now uses interior mutability
  behind `cfg(test)` and preserves an immutable production API.

### Final verification

Focused release verification:

- Receiver durable state: 63 passed, 0 failed.
- Python observation bridge: 8 passed, 0 failed.
- Observation privacy guard: 4 passed, 0 failed.
- OpenCode plugin: 7 passed, 0 failed.
- Agent observation reader and cursor: 21 passed, 0 failed.
- All-frontend adapter/controller contract: 6 passed, 0 failed.
- Durable App receiver behavior: 78 passed, 0 failed.
- Schema and startup migrations: 14 passed, 0 failed.
- Lifecycle hook integration: 22 passed, 0 failed.
- Completion and stop-hook continuity: 10 passed, 0 failed.

Repository-wide verification:

- `cargo test --release -- --test-threads=1`: 3,010 tests inventoried; all
  library, integration, and documentation targets passed. The library target
  passed 2,224 tests, 0 failed.
- `cargo clippy --release --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `cargo test --release --test module_structure`: 1 passed, 0 failed.
- `cargo test --release --test agent_registry_boundary`: 6 passed, 0 failed.
- `cargo test --release --test tui_receiver_runtime_architecture`: 4 passed,
  0 failed.
- `cargo test --release --test tui_receiver_dispatch_architecture`: 168 passed,
  0 failed.
- `cargo test --release --test tui_dependencies_architecture`: 8 passed,
  0 failed.
- `cargo test --release --test tui_state_aggregates_architecture`: 5 passed,
  0 failed.
- `cargo test --release --test tui_construction_boundary`: 3 passed, 0 failed.
- `cargo test --release --test access_boundary`: 6 passed, 0 failed.

The first full rerun after the duplicate-field GREEN passed all 2,224 library
tests, then the unchanged
`one_shared_process_routes_the_same_sender_to_two_exact_durable_queues` process
fixture missed its startup deadline. The exact test passed 1 of 1 and the whole
`receiver_workspace_isolation` target passed 24 of 24 immediately, with no code
change. The authoritative clean single-thread rerun then passed all 3,010
inventoried tests, including that target.

Final formatting, diff, changed-file, marker-limit, no-fixed-sleep, no-unsafe,
no-em-dash, provider-neutrality, and privacy audits passed. No
`docs/product-manager/**`, sealed review, or rereview artifact changed. The
bridge remains portable without unsafe Rust, the exact ten-field and 4096-byte
limits remain enforced, saturation still never reclaims, and BR-16, BR-17,
and BR-18 behavior remains excluded.

### Review fix changed files

- `Cargo.toml`
- `Cargo.lock`
- `scripts/receiver_observation_bridge.py`
- `src/tui/app_brain/receiver/active.rs`
- `src/tui/app_brain/tests/receiver_durable_diagnostics.rs`
- `src/tui/app_brain/tests/receiver_durable_observation_composed.rs`
- `src/tui/app_brain/tests/receiver_durable_observation_replacement.rs`
- `src/tui/app_brain/tests/receiver_durable_producer_matrix.rs`
- `src/tui/receiver/runtime.rs`
- `tests/fixtures/opencode/plugin_harness.js`
- `tests/receiver_observation_bridge.rs`
- `tests/receiver_observation_privacy.rs`
- `tests/stop_hook_actor/contracts.rs`
- `docs/architecture.md`
- `docs/features.md`
- `docs/integrations.md`
- `docs/data-model.md`
- `docs/decisions.md`
- `docs/testing.md`
- `.superpowers/sdd/BR-15-prove-receiver-job-acceptance-and-progress/task-5-report.md`

### Remaining concerns

No unresolved finding remains.

## Review fix round 2

### Evidence time and lease authorization

- RED: the exact-artifact precedence test supplied a schema-valid completed
  lifecycle boundary 60 seconds beyond the renewed lease. The job remained
  `launched`, proving that producer evidence time was also being used as lease
  authority.
- GREEN: after claim renewal and exact artifact/lifecycle validation, the App
  now samples a fresh clock for `authorized_at_unix_ms`. A validated completed
  boundary independently supplies `observed_at_unix_ms`; when that boundary is
  absent, the same fresh post-validation App time is the evidence fallback.
- The focused test now proves that the future producer timestamp is persisted
  exactly, fresh App time authorizes the commit, the response is delivered
  exactly once, the controller shuts down once, and the exact artifact is
  cleaned up.

### Generic privacy policy

- Mutation REDs proved that the prior curated guard accepted a private macOS
  home path, Unix home path, escaped Windows home path, email domain, and host
  literal in a newly discovered observation module. Additional REDs found that
  path-only future surfaces and a home identity with a generic-looking prefix
  could escape the first structural implementation.
- GREEN: recursive semantic and path discovery now audits observation and
  receiver-completion sources across Rust, Python, and JavaScript. The generic
  quoted-literal policy rejects non-generic macOS, Unix, and Windows home
  identities; email domains outside reserved test namespaces; and URL, host,
  and IP literals outside localhost, loopback, documentation networks, and
  reserved example namespaces. Only the guard's own policy and runtime-canary
  modules are excluded from self-audit.
- Runtime sender, local-path, and private-host canaries now cross direct submit,
  tool, stop-hook completion, OpenCode submit/tool/`session.idle`, Debug, error,
  diagnostic, log, and process-output paths. Normalized snapshots and emitted
  output retain none of them. Trusted observation artifacts retain only opaque
  identity fields, and trusted completion artifacts retain only opaque identity
  plus the intentional private final response.
- Runtime privacy characterization passed on its first behavioral run, so no
  production privacy failure was manufactured.

### Fix-round-2 verification

Focused release verification:

- Future producer evidence versus fresh lease authorization: 1 passed,
  0 failed.
- Receiver durable state: 63 passed, 0 failed.
- Agent observation reader and cursor: 21 passed, 0 failed.
- Durable App receiver behavior: 78 passed, 0 failed.
- Python observation bridge: 8 passed, 0 failed.
- Observation privacy policy and runtime canaries: 7 passed, 0 failed.
- OpenCode plugin: 7 passed, 0 failed.
- Lifecycle hook integration: 22 passed, 0 failed.
- Completion and stop-hook continuity: 10 passed, 0 failed.
- Schema and startup migrations: 14 passed, 0 failed.
- All-frontend adapter/controller contract: 6 passed, 0 failed.

Repository-wide verification:

- `cargo test --release -- --test-threads=1`: 3,013 tests inventoried and
  passed, including 2,224 library tests; 0 failed.
- `cargo clippy --release --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `cargo test --release --test module_structure`: 1 passed, 0 failed.
- `cargo test --release --test agent_registry_boundary`: 6 passed, 0 failed.
- `cargo test --release --test tui_receiver_runtime_architecture`: 4 passed,
  0 failed.
- `cargo test --release --test tui_receiver_dispatch_architecture`: 168 passed,
  0 failed.
- `cargo test --release --test tui_dependencies_architecture`: 8 passed,
  0 failed.
- `cargo test --release --test tui_state_aggregates_architecture`: 5 passed,
  0 failed.
- `cargo test --release --test tui_construction_boundary`: 3 passed, 0 failed.
- `cargo test --release --test access_boundary`: 6 passed, 0 failed.

Version 0.83.5 is recorded in both Cargo manifests. Final formatting, diff,
changed-file, structural-discovery, privacy, marker-limit, no-fixed-sleep,
no-unsafe, no-em-dash, and provider-neutrality audits passed. No
`docs/product-manager/**` or sealed review artifact changed. Saturation remains
no-reclaim, session continuity and exact cleanup remain intact, and BR-16,
BR-17, and BR-18 behavior remains excluded.

### Fix-round-2 changed files

- `Cargo.toml`
- `Cargo.lock`
- `src/tui/app_brain/receiver/active.rs`
- `src/tui/app_brain/tests/opencode_receiver.rs`
- `src/tui/state/services.rs`
- `tests/fixtures/opencode/plugin_harness.js`
- `tests/receiver_observation_privacy.rs`
- `tests/receiver_observation_privacy/policy.rs`
- `tests/receiver_observation_privacy/policy/literals.rs`
- `docs/architecture.md`
- `docs/features.md`
- `docs/integrations.md`
- `docs/data-model.md`
- `docs/decisions.md`
- `docs/testing.md`
- `.superpowers/sdd/BR-15-prove-receiver-job-acceptance-and-progress/task-5-report.md`

### Fix-round-2 remaining concerns

No unresolved finding remains.
