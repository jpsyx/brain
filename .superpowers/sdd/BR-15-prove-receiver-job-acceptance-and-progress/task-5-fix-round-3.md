# BR-15 Task 5 Fix Round 3 Report

Baseline: `137d29a8c37d92c08eb0c37567e50bdb9d146a08`

Review addressed: `task-5-rereview-2.md`

Version: `0.83.6`

## Scope

This round closes the two remaining Important findings without changing the
provider-neutral receiver contract:

1. A focused App test now crosses the real `agent_session_stop_hook.py`
   producer, its `receiver_observation_bridge.py` lifecycle write, the
   `AgentController`, and the receiver completion transaction while the App
   clock is intentionally behind the producer completion timestamp. The exact
   artifact commits once, the producer timestamp remains evidence, and fresh
   local App time remains lease authority.
2. The privacy policy now rejects a standalone non-reserved bare hostname from
   every recursively discovered path-based observation or receiver-completion
   producer based on the literal itself. It no longer depends on a finite list
   of variable, binding, or nearby context words. Semantic-only lifecycle
   modules retain their dotted external event identifiers without being
   misclassified as host literals.

The existing validation-after-expiry test remains the fail-closed proof. No
production completion, controller, store, delivery, cleanup, saturation, or
no-reclaim behavior changed in this round.

## RED and GREEN Evidence

### Context-free private-host policy

RED command:

```text
cargo test --release --test receiver_observation_privacy policy::newly_discovered_observation_sources_reject_private_home_email_and_host_literals -- --exact --nocapture
```

The mutation used the neutral declaration
`const VALUE: &str = "receiver.private.lan";`. Before the correction the test
failed with `privacy policy accepted a private host literal`, proving that the
finite context-word heuristic let the literal escape.

GREEN command:

```text
cargo test --release --test receiver_observation_privacy -- --nocapture
```

Result: 7 passed, 0 failed. The structural mutation now fails closed while the
reserved-host, filename, ordinary-code, runtime-canary, diagnostic, and trusted
opaque-identity cases remain green.

### Real-producer future-skew authorization

Characterization command against the corrected production path:

```text
cargo test --release --lib tui::app_brain::tests::receiver_durable_future_completion::stop_hook_future_completion_evidence_uses_fresh_local_lease_authority -- --exact --nocapture
```

Result: 1 passed, 0 failed. The test obtains both the strict Completed snapshot
and exact completion artifact through `agent_session_stop_hook.py`; it does not
use `write_completed_snapshot` as its producer.

For a genuine RED, the App authorization field was temporarily mutated to the
reviewed regression by passing the producer completion timestamp instead of
the freshly sampled App clock. The same exact command then failed with durable
phase `Launched` instead of `Done`. The mutation was immediately reverted.

Corrected GREEN command:

```text
cargo test --release --lib tui::app_brain::tests::receiver_durable_future_completion::stop_hook_future_completion_evidence_uses_fresh_local_lease_authority -- --exact --nocapture
```

Result: 1 passed, 0 failed. The producer timestamp is beyond the renewed lease
expiry, but completion is authorized with fresh local App time, persists the
exact producer timestamp, delivers exactly once, and cleans both producer
artifacts plus the active receiver resources.

Fail-closed retention command:

```text
cargo test --release --lib tui::app_brain::tests::receiver_durable_binding_completion::completion_validated_after_claim_expiry_cannot_finalize_or_run_terminal_effects -- --exact --nocapture
```

Result: 1 passed, 0 failed. When validation itself finishes after lease expiry,
the durable row and resources remain unchanged.

## Focused Verification

- `cargo test --release --lib tui::app_brain::tests::receiver_durable -- --test-threads=1`: 79 passed, 0 failed.
- `cargo test --release --test receiver_observation_privacy -- --nocapture`: 7 passed, 0 failed.
- `cargo test --release --test receiver_observation_bridge -- --nocapture`: 8 passed, 0 failed.
- `cargo test --release --test hook_integration -- --nocapture`: 22 passed, 0 failed.
- `cargo test --release --test stop_hook_actor -- --nocapture`: 10 passed, 0 failed.
- `cargo test --release --lib agent::observation -- --nocapture`: 21 passed, 0 failed.
- `cargo test --release --lib state::receiver -- --nocapture`: 63 passed, 0 failed.

Architecture targets remained green:

- `access_boundary`: 6 passed.
- `agent_registry_boundary`: 6 passed.
- `module_structure`: 1 passed.
- `tui_construction_boundary`: 3 passed.
- `tui_dependencies_architecture`: 8 passed.
- `tui_receiver_dispatch_architecture`: 168 passed.
- `tui_receiver_runtime_architecture`: 4 passed.
- `tui_state_aggregates_architecture`: 5 passed.

## Authoritative Gates

- `cargo test --release -- --test-threads=1`: 3,014 passed, 0 failed, 0 ignored.
- Library portion of the authoritative suite: 2,225 passed, 0 failed.
- `cargo clippy --release --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- The structural privacy guard recursively discovers current and future
  observation and receiver-completion surfaces, and all 7 policy/canary tests
  passed.

## Documentation and Version

- `docs/integrations.md` states the exact structural privacy scope and the
  distinct evidence-time and authorization-time contract.
- `docs/testing.md` names the real stop-hook producer proof, the context-free
  neutral-binding mutation, and the retained fail-closed expiry proof.
- `Cargo.toml` and `Cargo.lock` moved together from `0.83.5` to `0.83.6`.

## Unresolved Findings

None. Critical 0, Important 0, Minor 0 within this fix-round scope.
