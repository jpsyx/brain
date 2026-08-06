# Task 7 report: multi-workspace acceptance harness

## Status

PASS. The composed acceptance harness, gated two-workspace local-rclone
coverage, watcher lifecycle coverage, durable documentation, release tests,
and strict lint gate are complete. Local commit `af1c502` contains the work;
nothing was pushed.

## Version and base

- Starting version: `0.34.0`
- Final version: `0.34.1`
- Starting commit: `f4f69a8398405197472b84ed317917fcbbacd389`

## Outcome

`tests/multi_workspace_acceptance.rs` now composes the existing production
workspace, receiver, actor, task, merge, triage, capability, agent-controller,
lock, and shared-server seams in one hermetic personal-plus-family lifecycle.
The provider request and agent process transport remain fake external edges.
No ratatui terminal, PTY, cloud provider, production remote, or production user
state is opened.

The scenario proves all eleven rollout assertions:

1. Personal and family have distinct workspace UUIDs and UUID-derived caches.
2. Personal access is unrestricted while family access is workspace-only.
3. Omitted CLI selection resolves personal and `-b fam` resolves family.
4. Both fake TUIs hold distinct locks and register two leases in one shared
   server process.
5. Authenticated family SMS resolves to portable actor `wife`; a fake agent
   transport invokes the real Brain-owned task script and verifies the created
   task is assigned to `wife`.
6. Personal and family sync locks can be held concurrently while a second
   family lock is refused.
7. Independent `T7` rows merge by UUID, deterministically renumber the second
   row to `T8`, and converge independently of mirror order.
8. Family triage is disabled; disabled completion returns `Disabled`, creates
   no habits/history file, and requirements report both managed triage and the
   triage modal as `Off`.
9. Family workspace-only capability resolution carries the family root and
   actor provenance, excludes personal credentials, and produces equivalent
   Claude and Codex launches through `AgentController`.
10. Closing family removes its lease and returns family-unavailable behavior
    while the personal receiver remains live.
11. Closing personal removes the final lease, terminates the shared server,
    and removes its server state within the fixture's bounded deadline.

## Local transport and watcher coverage

`tests/sync_local.rs` is now a 91-line shared harness with cohesive modules in
`tests/sync_local/` for transport, CSV merge, conflicts, multi-workspace
behavior. The gated multi-workspace case runs two local rclone remotes
concurrently through distinct production `WorkspacePaths`, verifies separate
bisync workdirs and semantic CSV baselines, then presents a personal local
workspace with a family remote manifest. The UUID mismatch is refused before
the bisync workdir exists or local content reaches the remote.

`tests/watch_local.rs` uses temporary roots only. It starts personal and family
watchers, joins one worker, and proves the peer still fires. Channels and
bounded deadlines replace fixed sleeps in lifecycle assertions.

## RED and GREEN evidence

### Composed acceptance RED

Command:

```text
cargo test --release --test multi_workspace_acceptance -- --nocapture
```

The first run failed at the authenticated actor seam:

```text
assertion `left == right` failed
  left: "family-member"
 right: "wife"
```

The next run reached the real task script and exposed missing normal workspace
fixture provisioning:

```text
FileNotFoundError: .../family/tasks/.tasks_next_id
```

The test setup then wrote portable users/config into the temporary registry,
mapped the signed family sender to `wife`, and provisioned each temporary
workspace's ordinary `tasks/` directory. No production branch or
acceptance-only application path was added. The focused test passed.

### Two-workspace rclone RED

Command:

```text
cargo test --release --test sync_local concurrent_local_remotes_use_distinct_workspace_runtime_paths -- --nocapture
```

Both local resyncs completed, but the family workdir assertion failed. The old
integration helper constructed one fixed personal workspace ID internally, so
the test could not drive two production `WorkspacePaths`. The helper was made
explicitly workspace-scoped while retaining its old single-workspace wrapper.
The focused case and the full seven-test rclone integration suite passed.

### Strict Clippy finding

The first strict Clippy run found two `assigning_clones` warnings in the new
fixture setup. Both assignments now use `clone_into`; the acceptance test and
strict Clippy rerun passed.

### Receiver cleanup race fixed during final verification

The first final full-suite rerun reproduced the previously deferred macOS
receiver cleanup race in
`failed_ack_write_rolls_back_the_just_enqueued_job`:

```text
called `Result::unwrap()` on an `Err` value:
Os { code: 57, kind: NotConnected, message: "Socket is not connected" }
```

The functional rollback assertions were not implicated. The peer could finish
and close between the client commit and the test-side `shutdown(Both)`, making
`NotConnected` an equally valid terminal cleanup state. The test now accepts
only that error kind while continuing to reject every unexpected shutdown
error. No production code or sleep was added. The exact test passed twenty
consecutive runs, the complete receiver integration passed 21 tests, and the
subsequent full release suite passed.

## Durable documentation

Updated:

- `docs/testing.md`
- `docs/architecture.md`
- `docs/integrations.md`
- `docs/data-model.md`
- `docs/decisions.md`

The docs describe the composed acceptance boundary, two-workspace local
transport complement, UUID-owned baseline/workdir evidence, lifecycle polling,
and the focused integration module tree.

## Final verification

- `cargo test --release --test multi_workspace_acceptance -- --nocapture`:
  PASS, 1 test.
- `cargo test --release --test sync_local -- --nocapture`: PASS, 7 tests;
  rclone was available and executed.
- `cargo test --release --test watch_local -- --nocapture`: PASS, 2 tests.
- `cargo test --release`: PASS, including 1,299 library unit tests and all
  integration/doc tests.
- Twenty consecutive exact runs of
  `failed_ack_write_rolls_back_the_just_enqueued_job`: PASS, 20/20.
- `cargo test --release --test receiver_workspace_isolation`: PASS, 21 tests.
- `cargo clippy --release --all-targets -- -D warnings`: PASS.
- `cargo test --release bundled_skills_carry_no_personal_data`: PASS.
- `python3 -m unittest discover -s skills/todo/scripts/tests`: PASS, 23 tests.
- `rustfmt --edition 2024 --check` over every Task 7 Rust file: PASS.
- `git diff --check`: PASS.

## Deferred minor finding

Repo-wide `cargo fmt --check` still reports extensive formatting drift in
files inherited from earlier branch tasks. No Task 7 Rust file is among the
reported drift after its focused rustfmt check. The exact Task 7 base commit
already yields the same representative formatter diff at `src/cli/tasks.rs:186`,
which confirms the drift predates this task. Task 8's branch-wide release audit
must resolve or account for this formatting state before the final rollout
handoff.

## Boundary

The acceptance scenario intentionally does not exercise a production remote,
real provider credentials, a live TUI, or a real agent PTY. The local-rclone
complement uses only temporary directories and matching or deliberately
mismatched temporary manifests. OpenCode remains inert; Claude and Codex are
the supported equivalent lifecycle surfaces through `AgentController`.
