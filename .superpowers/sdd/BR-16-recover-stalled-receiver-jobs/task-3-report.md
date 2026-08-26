# BR-16 Task 3 Report

## Status

DONE

## Summary

Task 3 turns BR-16's persisted lifecycle facts and pure recovery policy into an
atomic, restart-safe store reconciliation boundary without adding the App,
frontend, or provider effects reserved for Task 4. The change:

- scans the oldest blocking nonterminal receiver job under one immediate
  transaction, evaluates the pure policy, and applies at most one exact
  full-snapshot transition;
- maps an unaccepted timeout to the existing five-second bounded ordinary
  retry, clears the superseded owner, registration, and current-attempt cursor,
  and leaves the accepted-recovery budget untouched;
- terminalizes exhausted preacceptance work, absolute or recovery expiry,
  missing exact native-session evidence, unsafe claimed recovery resume, and
  incomplete legacy completion with a stable reason plus durable pending-notice
  intent;
- persists the first accepted stall as one ownerless due recovery, preserves
  immutable job, inbound, conversation, frontend, native binding, absolute
  deadline, and lifetime evidence, and spends the recovery budget exactly at
  reconciliation;
- narrows the provisional Task 1 recovery claim so it can claim only an
  already-persisted due recovery, establishes only its launch deadline, and
  cannot rediscover stale accepted work or increment the recovery counter;
- lets recovery discovery survive store reopen while ordinary FIFO claiming
  refuses to skip or consume a due recovery;
- compares token, state, owner and expiry, retry facts, remote/native identity,
  current-attempt evidence, every lifecycle deadline, recovery count, attempt
  kind, pending-notice intent, and update timestamp before committing; and
- returns only frontend-neutral, content-free action, reason, job, token, and
  cleanup-instance identifiers for Task 4.

The crate version moved from 0.84.6 to 0.84.7.

## RED evidence

Production changes followed focused failures for each behavior slice.

### Reconciliation API and preacceptance retry

Command:

```text
cargo test --release state::receiver::tests::reconciliation -- --test-threads=1
```

Observed failure excerpt:

```text
error[E0599]: no method named `reconcile_next_receiver_job` found for struct `state::Db`
error[E0433]: failed to resolve: use of undeclared type `ReceiverReconciliationAction`
error[E0433]: failed to resolve: use of undeclared type `ReceiverReconciliationReason`
```

The store had no atomic oldest-blocker transition or neutral effect model.

### Persist recovery before claim

Command:

```text
cargo test --release \
  state::receiver::tests::reconciliation::reconciliation_persists_one_ownerless_same_session_recovery_before_claim \
  -- --exact --test-threads=1
```

Observed failure excerpt:

```text
assertion failed: fixture.db
    .claim_receiver_recovery_run(...)
    .expect("direct stale-work claim is rejected")
    .is_none()
```

Task 1's provisional seam still jumped directly from accepted stale work to a
claimed recovery and spent the recovery budget at claim time.

### Terminal policy mappings and durable notice intent

Command:

```text
cargo test --release state::receiver::tests::reconciliation -- --test-threads=1
```

Observed result:

```text
test result: FAILED. 3 passed; 4 failed
```

The missing-native-session, exhausted-preacceptance, absolute-expiry, and
incomplete-legacy cases returned no terminal effect because only the
preacceptance and accepted-recovery transitions existed.

### Reopen recovery discovery

Command:

```text
cargo test --release \
  state::receiver::tests::reconciliation::due_recovery_survives_reopen_and_is_discovered_before_later_fifo_work \
  -- --exact --test-threads=1
```

Observed failure excerpt:

```text
error[E0599]: no method named `claim_next_receiver_recovery_run` found for struct `state::Db`
```

The store could claim only a caller-selected job ID and had no restart-safe due
recovery discovery seam.

### Unsafe claimed recovery resume

Command:

```text
cargo test --release \
  state::receiver::tests::reconciliation::claimed_recovery_with_unsafe_native_history_terminalizes_durably \
  -- --exact --test-threads=1
```

Observed failure excerpt:

```text
error[E0599]: no method named `fail_receiver_recovery_resume` found for struct `state::Db`
```

Task 4 had no exact-owner store seam for persisting fail-closed native-history
validation before later notice delivery.

### Race characterization

The full-snapshot implementation existed by the time the duplicate-reconciler,
completion-wins, and stale-writer characterization cases were added. Those
tests passed on their first run. They are recorded as green race validation,
not represented as RED evidence.

## GREEN and refactor evidence

### Focused Task 3 tests

Commands and results:

```text
cargo test --release --lib state::receiver::tests::reconciliation -- --test-threads=1
test result: ok. 14 passed; 0 failed

cargo test --release --lib state::receiver::tests -- --test-threads=1
test result: ok. 99 passed; 0 failed

cargo test --release --lib state::receiver::tests::recovery_claim -- --test-threads=1
test result: ok. 3 passed; 0 failed

cargo test --release --test startup_migration receiver_model -- --test-threads=1
test result: ok. 9 passed; 0 failed
```

The reconciliation module covers exact-boundary wait, live-owner and
expired-owner preacceptance timeout, exhaustion and FIFO advancement, first and
second accepted stalls, absolute and recovery expiry, exact native binding,
unsafe or missing resume evidence, incomplete legacy completion, ownerless
recovery persistence, reopen discovery, full-snapshot duplicate suppression,
completion winning first, and fencing of late progress, completion, renewal,
and process-exit writes.

### Complete uninterrupted release suite

Command:

```text
cargo test --release -- --test-threads=1
```

Result:

```text
exit code: 0
library: 2277 passed; 0 failed
all integration test binaries passed
doc-tests: 0 passed; 0 failed
```

This was one uninterrupted invocation. It completed without retrying or
excluding a target.

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

## Files changed

- Release metadata: `Cargo.toml`, `Cargo.lock`.
- Neutral model and exports: `src/state/mod.rs`,
  `src/state/receiver/{mod.rs,model.rs}`.
- Reconciliation transaction:
  `src/state/receiver/store/reconciliation.rs` and
  `src/state/receiver/store/reconciliation/{support,terminal}.rs`.
- Claim narrowing and discovery:
  `src/state/receiver/store/claim/{next,recovery}.rs` plus
  `src/state/receiver/store.rs`.
- Focused unit coverage: `src/state/receiver/tests.rs`,
  `src/state/receiver/tests/recovery_claim.rs`,
  `src/state/receiver/tests/reconciliation.rs`, and
  `src/state/receiver/tests/reconciliation/{support,preacceptance,recovery,terminal,races}.rs`.
- Contract documentation:
  `docs/{architecture,data-model,decisions,features,integrations,testing}.md`.
- Delivery evidence: this report.

## Self-review

- Verified the policy read and exact transition share one immediate
  transaction and the guarded update covers every brief-named mutable fact.
- Verified equality expires every lifecycle boundary at `now >= expires_at`;
  claim expiry remains only a writer fence and cannot replace or renew a
  lifecycle deadline.
- Verified the first accepted stall persists one recovery before any claim,
  increments only `recovery_count`, and keeps the job token, inbound response
  identity, conversation, exact frontend/native binding, immutable absolute
  deadline, and first accepted/progress facts.
- Verified ordinary claim cannot consume a due recovery, recovery discovery
  survives reopen, and the recovery claim no longer evaluates accepted stale
  work or spends the recovery budget.
- Verified terminal state and pending unavailable-notice intent commit together
  before any Task 4 cleanup or provider operation.
- Verified the winning reconciliation fences the superseded instance's late
  observation, completion, renewal, and launch-failure transitions, while a
  completed exact session prevents later reconciliation.
- Verified semantic effects contain no sender, recipient, message, attachment,
  answer, transcript, provider credential, or provider event grammar.
- Verified no App tick, controller, prompt, frontend adapter, provider reply,
  or notice-dispatch code changed in this task.
- Verified production and test files remain focused and below the repository's
  approximate 400-line modularity review threshold.

## Deferred Task 1 minors

- The schema-v10 migration split remains deferred to Task 5. Task 3 did not
  touch schema migration behavior, and moving the already-reviewed up, repair,
  and down paths would add unrelated migration risk to a store-only change.
- The duplicated progress-deadline clamp remains deferred to Task 5. Task 3
  consumes the persisted result but does not change observation derivation or
  schema SQL. Task 2 already covers the live observation writer's saturation
  and absolute-limit behavior; Task 5 can consolidate or directly cross-test
  the migration and live SQL seams without widening this transaction slice.

## Concerns

None within Task 3. App ordering, exact `AgentController` native-history
validation and resume, local cleanup, recovery prompt construction, and notice
dispatch remain intentionally assigned to Task 4.

## Commit

The focused Task 3 product commit contains the 0.84.7 release metadata, source,
tests, docs, and this report. Its final hash is supplied in the task handoff
because embedding it here would change that hash.

## Fix round 1 of 5

### Status

DONE

This round resolves every finding in `task-3-review.md` and supersedes the
original report wherever the behavior changed. The crate version moved from
0.84.7 to 0.84.8.

### Behavior corrected

- An ownerless recovery remains in the recurring reconciliation scan. Reopen
  tests prove it waits one millisecond below each lifetime boundary, then
  terminalizes with pending notice intent at equality and afterward for both
  recovery and absolute expiry.
- Targeted and discovered recovery claims now recheck the workspace's globally
  oldest claimable or blocking row under the same immediate transaction. An
  older expired-owner lifecycle row or due ordinary retry retains FIFO
  priority.
- The accepted-stall transition persists an all-or-none cleanup fence containing
  the exact superseded instance and native session. It retains the matching
  registration until the exact job, token, snapshot, instance, session, and
  registration are acknowledged through one immediate transaction. Recovery
  claim guards require both fence fields to be clear.
- `record_receiver_launch_retry` now accepts ordinary attempts only. The exact
  live recovery owner has a separate content-free terminal seam for planning,
  registration, spawn, and shutdown failure; every category sets stable reason
  metadata and pending notice intent without recycling or respending recovery.
- A real file-backed SQLite characterization holds one immediate writer lock,
  starts a second reconciler through a barrier on a separate handle with zero
  busy timeout, observes the actual lock error, then proves the second handle
  publishes exactly one effect after the first transaction rolls back.
- Schema-v10 column installation, v9 migration, current-schema repair, and
  v10-to-v9 downgrade now live in focused `schema/recovery.rs`. Automatic
  startup still installs both cleanup columns; downgrade removes them, and a
  partial cleanup tuple repairs fail-closed to terminal notice intent.

App cleanup, acknowledgement ordering, native-history validation, controller
resume, recovery prompt construction, and provider notice delivery remain Task
4 effects. No App, frontend adapter, hook, server/provider, or delivery code
changed.

### RED evidence

#### Ownerless recovery lifetime bounds

Command:

```text
cargo test --release --lib state::receiver::tests::reconciliation::ownerless_due_recovery_terminalizes -- --test-threads=1 --nocapture
```

Observed failures:

```text
left: None
right: Some(RecoveryExpired)

left: None
right: Some(AbsoluteWorkExpired)
```

The retrying recovery was absent from the candidate scan and the pure policy
returned `Wait` for every retrying row.

#### Global recovery FIFO gate

Commands:

```text
cargo test --release --lib state::receiver::tests::recovery_claim::targeted_recovery_claim_refuses_every_older_workspace_blocker -- --test-threads=1 --nocapture
cargo test --release --lib state::receiver::tests::recovery_claim::recovery_discovery_refuses_every_older_workspace_blocker -- --test-threads=1 --nocapture
```

Observed failures:

```text
targeted recovery skipped an older blocker for targeted-expired
recovery discovery skipped an older blocker for discovery-expired
```

Both seams considered only the target recovery subset rather than the globally
oldest workspace row.

#### Exact cleanup-pending fence

Command:

```text
cargo test --release --lib state::receiver::tests::reconciliation::second_tui_cannot_claim_recovery_until_exact_cleanup_is_acknowledged -- --test-threads=1 --nocapture
```

Observed compilation failures included missing
`cleanup_session_id`, `recovery_cleanup_instance`,
`recovery_cleanup_session_id`, and
`acknowledge_receiver_recovery_cleanup` APIs. The store had no durable fence or
acknowledgement transition. After the APIs existed, the schema-repair RED was:

```text
cargo test --release --lib state::receiver::tests::schema::current_v10_repair_terminalizes_a_partial_recovery_cleanup_fence -- --test-threads=1 --nocapture

left: Retrying
right: Failed
```

#### Recovery launch and shutdown failures

Command:

```text
cargo test --release --lib state::receiver::tests::reconciliation::ordinary_launch_retry_seam_rejects_every_recovery_failure_category -- --test-threads=1 --nocapture
```

Observed failure on the first table row:

```text
left: Some(Scheduled)
right: None
failure category: Planning
```

The follow-on exact transition test initially failed to compile because
`ReceiverRecoveryFailure`, its four stable reasons, and
`fail_receiver_recovery_attempt` did not exist:

```text
cargo test --release --lib state::receiver::tests::reconciliation::every_claimed_recovery_launch_failure_terminalizes_with_notice_intent -- --test-threads=1 --nocapture
```

#### Real interleaving characterization

The deterministic lock test passed on its first run after the immediate
transaction implementation already existed. This is honest concurrency
characterization, not reconstructed historical RED evidence:

```text
cargo test --release --lib state::receiver::tests::reconciliation::immediate_writer_lock_serializes_real_separate_handle_reconcilers -- --test-threads=1 --nocapture
test result: ok. 1 passed; 0 failed
```

### GREEN and refactor evidence

Focused commands and final results:

```text
cargo test --release --lib state::receiver::tests::reconciliation -- --test-threads=1
test result: ok. 20 passed; 0 failed

cargo test --release --lib state::receiver::tests::recovery_claim -- --test-threads=1
test result: ok. 5 passed; 0 failed

cargo test --release --lib state::receiver::tests -- --test-threads=1
test result: ok. 109 passed; 0 failed

cargo test --release --test startup_migration receiver_model -- --test-threads=1
test result: ok. 9 passed; 0 failed
```

The required uninterrupted release suite completed in one invocation:

```text
cargo test --release -- --test-threads=1
library: 2287 passed; 0 failed
all integration test binaries passed
doc-tests: 0 passed; 0 failed
```

Final gates:

```text
cargo clippy --release --all-targets -- -D warnings
Finished `release` profile; exit code 0

cargo fmt --all -- --check
exit code 0

git diff --check
exit code 0
```

### Files and self-review

- Added the cleanup tuple, typed recovery failure categories, exact terminal
  reasons, ownerless expiry policy, global recovery ordering, and cleanup
  acknowledgement to the state model and store.
- Added focused recovery cleanup, expiry, failure, FIFO ordering, real lock,
  schema repair, startup up, and downgrade coverage. Large touched test groups
  were split into behavior-owned files below the approximate 400-line smell
  threshold.
- Split schema-v10 recovery migration code into
  `src/state/receiver/schema/recovery.rs`; the parent schema coordinator is now
  396 lines and the focused child is 277 lines.
- Updated all six Task 3 contract documents and release metadata.
- Verified the exact snapshot includes both cleanup fields, failed CAS paths
  roll back registration release, terminal state and notice intent commit
  together, recovery count remains one, and metadata remains content-free.
- Verified ownerless recovery expiration cannot leave later FIFO work blocked,
  and recovery claiming cannot cross an older expired-owner lifecycle, due
  ordinary retry, live claim, or cleanup fence.

The deferred progress-deadline clamp consolidation remains assigned to Task 5.
This round did not change either the live observation deadline SQL or migration
deadline derivation. Existing cross-tests continue to cover saturation,
future-skew, and absolute-limit clamping on both paths; consolidating the SQL is
not required for the reviewed fixes and would widen this store transition
round.

### Concerns

None within the Task 3 store boundary. Task 4 must perform the native cleanup
before calling the new exact acknowledgement seam and must route every recovery
planning, registration, spawn, or shutdown failure through the typed terminal
transition.

### Commit

The focused fix commit contains the 0.84.8 release metadata, source, tests,
docs, and this appended report. Its final hash is supplied in the handoff
because embedding it here would change that hash.
