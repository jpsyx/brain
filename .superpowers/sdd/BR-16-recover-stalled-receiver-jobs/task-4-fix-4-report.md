# BR-16 Task 4 fix round 4 report

## Scope and base

- Exact base: `05a84f39ac7c5d402aaa4c1d8009edb902592502`.
- Scope: retain exact recovery authority across every pre-spawn store ambiguity,
  successful-spawn activation cut, and shutdown-first cleanup cut identified by
  `task-4-rereview-3.md`.
- Preserved the prior Task 4 `T/P/I/S` attribution, exact cleanup
  acknowledgement, disabled no-spawn behavior, schema-v11 serialization,
  one-notice behavior, and frontend-neutral `AgentController` path.
- No BR-17 provider delivery behavior, BR-18 cutover behavior, dependency, or
  `docs/product-manager/**` file changed.

## Observed RED

The new tests were written and observed failing before their production slices.

### Slice 1: pre-spawn authority

```text
cargo test --release --lib tui::app_brain::tests::receiver_recovery_authority:: -- --test-threads=1
```

The first boundary matrix ran six cases: five failed and one existing final
authorization case passed. Store errors after capability planning, frontend
availability, resume validation, registration, and launch preparation left the
App idle instead of retaining the exact recovery claim. The dedicated cleanup
uncertainty RED then failed because the injected shutdown failure was ignored:
the transport recorded one shutdown where the test required zero and no local
cleanup authority survived.

### Slice 2: successful-spawn authority

```text
cargo test --release --lib tui::app_brain::tests::receiver_recovery_spawn_authority:: -- --test-threads=1
```

Both tests failed. Store and launch-commit ambiguity discarded the spawned
controller instead of retaining one capability. The cleanup matrix also proved
the old path erased the injected shutdown failure and lost the exact controller
and registration fence.

## GREEN implementation

### Pre-spawn state and decisions

Owner checks now return `Current`, `Lost`, or `StoreUnavailable`.

| Decision or cut | Local state | Next action |
| --- | --- | --- |
| `Current` | existing `RecoveryClaimed` capability continues | execute only the next recovery boundary |
| `StoreUnavailable` before registration | exact `RecoveryClaimed` value | retry the same persisted recovery attempt |
| `StoreUnavailable` after controller or registration creation | `RecoveryPreSpawnCleanup` with controller and optional exact attribution | prove shutdown and exact release, then restore the same recovery claim |
| proven `Lost` | cleanup capability when resources exist | prove shutdown and release, then do not restore the old claim |
| genuine planning, availability, validation, or registration failure | typed cleanup outcome | preserve the existing bounded durable failure semantics |
| launch preparation `Err` | exact pre-spawn cleanup and `RestoreClaim` | retry without entering ordinary Fresh planning |
| launch preparation `Ok(false)` | exact pre-spawn cleanup and `Lost` | never retry the old claim |

Every retry retains the original job, token, claim owner, workspace, actor,
channel, frontend, native session, and recovery attempt. It never enters
attachment staging, `/new`, transcript replay, inbound prompt construction, or
current-default-frontend selection.

### Successful-spawn capability

`AgentController::launch` success immediately commits the exact registration
guard into `RecoverySpawned`. That local capability owns the complete claimed
run, controller or exact receiver tab, registration attribution, native
session, scope, frontend, PID, durable-commit status, cleanup effect, and
completed cleanup steps.

The stages are:

1. `PostSpawnOwner(controller)`: retry owner proof and the exact durable launch
   commit without spawning again.
2. `PostAllocationOwner(tab_id)`: the exact tab owns the controller while final
   owner proof is retried.
3. `CleanupDetached(controller)`: retry shutdown of an unallocated controller.
4. `CleanupTabbed(tab_id)`: retry shutdown of the exact tab-owned controller.

An ambiguous launch-commit `Err` retains `PostSpawnOwner`. A later exact commit
retry distinguishes the two durable worlds: a successful new write returns
`Ok(true)`, while `Ok(false)` reopens and verifies the exact visible Launched
job/token/instance/session evidence. Only a proved mismatch enters cleanup.

Tab capacity is reserved before controller ownership moves. Exact insertion is
infallible after reservation, so an already-running or exhausted allocation
leaves the controller available for cleanup. Generic skill-session and human
panel behavior remains unchanged.

Cleanup is strictly ordered: controller shutdown, exact tab removal, exact
artifact removal, durable terminal cleanup establishment, then exact cleanup
acknowledgement or exact registration release. Any failed step retains the
capability and retries only that unfinished action. Reconciliation can attach
the exact matching cleanup effect, but cannot release the registration or
native-session lock while local shutdown remains unproved.

## Crash and restart matrix

| Cut | Durable evidence | Local authority | Resolution |
| --- | --- | --- | --- |
| pre-spawn owner-store error | live recovery claim | `RecoveryClaimed` or `RecoveryPreSpawnCleanup` | exact cleanup, then same recovery retry |
| pre-spawn cleanup error | live recovery claim and registration if created | controller plus exact optional attribution | retry cleanup; no competing claim |
| post-spawn owner-store error | Launching recovery plus committed registration | `PostSpawnOwner(controller)` | retry owner proof, no respawn |
| launch commit `Err` before write | Launching recovery | `PostSpawnOwner(controller)` | retry exact commit |
| launch commit `Err` after visible write | exact Launched observation | `PostSpawnOwner(controller)` | reopened exact proof, then allocation |
| launch commit `Ok(false)` or owner loss | proved changed durable owner/state | `CleanupDetached(controller)` | shutdown first, then exact release/terminal cleanup |
| allocation failure | exact Launched evidence | `CleanupDetached(controller)` | shutdown first, then exact durable acknowledgement |
| final owner-store error | exact Launched evidence | `PostAllocationOwner(tab_id)` | retry owner proof with the controller still in the exact tab |
| final owner loss | exact Launched evidence | `CleanupTabbed(tab_id)` | shutdown exact tab first, then acknowledge cleanup |
| shutdown or later cleanup failure | exact registration/session fence remains | same cleanup stage plus completed-step flags | retry only unfinished work; FIFO and competing session claims remain blocked |
| TUI process crash | persisted job, registration, PID, and attribution remain | no new TUI may reap a live recorded PID | existing exact dead-PID restart proof completes stale cleanup after process death |

Prior binding `T`, placeholder `P`, instance `I`, native session `S`, unrelated
registrations and locks, and later FIFO work remain unchanged until the exact
cleanup succeeds.

## Module refactor

The receiver recovery and unavailable-notice facades moved from the catch-all
`AppServices` implementation into:

- `src/tui/state/services/receiver_recovery.rs` (117 total lines);
- `src/tui/state/services/receiver_notice.rs` (104 total lines).

`src/tui/state/services.rs` is 510 total lines, with its inline test module
starting at line 408. Recovery launch behavior is split into claim,
pre-spawn-cleanup, activation, and cleanup modules:

- `recovery_launch.rs`: 403 total production lines, only three lines above the
  approximate 400-line smell threshold and not materially over it;
- `claim.rs`: 33 lines;
- `pre_spawn_cleanup.rs`: 93 lines;
- `effects.rs`: 59 lines;
- `effects/activation.rs`: 170 lines;
- `effects/cleanup.rs`: 140 lines.

The structural aggregate inventory was updated for the new test-only service
boundary field and passes its complete five-test architecture suite.

## Verification

Passed:

```text
cargo fmt --all -- --check
cargo test --release --lib tui::app_brain::tests::receiver_recovery_authority:: -- --test-threads=1
# 7 passed
cargo test --release --lib tui::app_brain::tests::receiver_recovery_spawn_authority:: -- --test-threads=1
# 2 passed
cargo test --release --lib state::receiver::tests:: -- --test-threads=1
# 122 passed
cargo test --release --lib tui::app_brain::tests::receiver -- --test-threads=1
# 117 passed
cargo test --release --test startup_migration -- --test-threads=1
# 20 passed
cargo test --release --test state_concurrency -- --test-threads=1
# 2 passed
cargo test --release --test receiver_observation_privacy -- --test-threads=1
# 12 passed
cargo test --release --test agent_registry_boundary -- --test-threads=1
# 6 passed
cargo test --release --test tui_state_aggregates_architecture -- --test-threads=1
# 5 passed
cargo test --release --test receiver_workspace_isolation -- --test-threads=1
# 24 passed
cargo clippy --release --all-targets -- -D warnings
git diff --check
```

The exact required full serial command was run repeatedly. Every run passed all
2,324 library tests. The first reached the late structural aggregate suite and
found the now-corrected test-only field inventory; all preceding integration
suites, including `receiver_workspace_isolation` 24/24, were green. Three later
runs each hit the same preexisting three-second process-fixture timeout in a
different `receiver_workspace_isolation` case. Each timed-out exact case passed
immediately by itself, and the complete isolation suite passed 24/24 twice
afterward. No changed file touches that ingress process fixture. Therefore the
required command was executed but did not produce one monolithic green exit;
the full changed behavior, all policy gates, the corrected structural target,
and the timing-sensitive target all pass independently.

## Privacy and boundary scans

- No added `unsafe` or fixed sleep appears in changed Rust production or test
  files.
- Changed production code constructs no concrete Claude, Codex, or OpenCode
  controller. Every frontend remains behind `AgentController`.
- Privacy and registry-boundary policy suites pass 12/12 and 6/6.
- Added diagnostics contain stable boundary labels only, not prompt, answer,
  transcript, sender, recipient, attachment, subject, payload, or credential
  content.
- `Cargo.toml` and `Cargo.lock` change only the Brain package version.
- No `docs/product-manager/**` file changed.

## Docs, version, and commit

Updated `docs/architecture.md`, `docs/data-model.md`, `docs/features.md`,
`docs/integrations.md`, and `docs/testing.md` for the exact pre-spawn retry,
successful-spawn capability, shutdown-first cleanup, crash/restart behavior,
and deterministic regression coverage. Bumped the crate compatibly from
`0.84.15` to `0.84.16` in `Cargo.toml` and `Cargo.lock`.

The final commit hash is returned in the controller handoff. A Git commit cannot
contain its own final object hash because that hash includes this report's
contents.

## Concerns

The receiver process fixture's fixed three-second startup poll is intermittently
insufficient after the monolithic serial suite. This is preexisting and outside
BR-16 Task 4 fix round 4; its exact target remains green standalone.
