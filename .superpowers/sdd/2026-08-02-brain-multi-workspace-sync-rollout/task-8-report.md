# Task 8 report: durable docs, release surfaces, and final verification

## Status

PASS. All required durable docs and release surfaces describe the completed
Phase 5 module tree and CLI. The final compatible version after review fixes is
`0.35.3`. All release,
acceptance, privacy, read-only, lint, skill, help, and smoke gates passed. The
branch remains local and the worktree is preserved.

## Version and base

- Starting version: `0.34.1`
- Final version: `0.35.2`
- Starting commit: `af1c502d72f3395c08fe18047aae64a4b5ac56d3`
- Task commit: this local Task 8 commit

## RED evidence

The first release-surface audit found four concrete failures before the final
changes:

1. `Cargo.toml` and `Cargo.lock` still reported `0.34.1`, below the required
   additive Phase 5 minor release.
2. `cargo test --release --test workspace_docs -- --nocapture` failed two
   assertions because its old security contract still required
   `prompt-based guidance` and `not a filesystem sandbox` instead of the exact
   Phase 5 advisory warning.
3. A new focused help assertion failed because compiled root help still said
   `Alt-? shows help`, while the implemented and documented binding is `Alt-S`.
4. A new Cargo-metadata assertion failed because the package description still
   described one `~/brain` root and a Claude-only handoff.

Each failing assertion was observed before its corresponding production or
release-surface change. Focused reruns then passed.

## Durable documentation and release surfaces

Updated `README.md`, every Task 8 durable document, and `AGENTS.md` to describe:

- immutable `WorkspaceContext`, `ActorContext`, and `AgentController` flow;
- UUID-owned sync locks, journals, workdirs, baselines, freshness, triggers,
  capability state, migration state, and backups;
- selected-workspace-only sync configuration and strict remote manifest
  identity, including exact UUID adoption for a nonempty legacy remote;
- detached canonical workspace selection with UUID consistency checks;
- explicit, journaled, resumable, backed-up, atomic, idempotent migration with
  final legacy semantic sync before UUID task identity becomes authoritative;
- required availability versus independently optional `off`, `ready`, and
  `incomplete` features;
- the composed personal-plus-family acceptance boundary and local-rclone
  complement;
- the shared TUI-lifetime server, selected receiver surfaces, Claude/Codex
  parity, and OpenCode's fail-fast stub;
- the exact security statement that `workspace_only` is advisory prompt
  enforcement plus best-effort capability filtering, easy to bypass, and not
  tenant isolation.

The package description, version output, root help binding, docs-contract
module paths, smoke commands, test commands, and `Cargo.lock` were reconciled
with the finished tree.

## Verification

- `cargo test --release`: PASS, 1,299 library tests plus every integration and
  doc test.
- `cargo clippy --release --all-targets -- -D warnings`: PASS.
- `cargo test --release bundled_skills_carry_no_personal_data`: PASS.
- `python3 -m unittest discover -s skills/todo/scripts/tests`: PASS, 23 tests.
- `cargo test --release --test multi_workspace_acceptance`: PASS, 1 test.
- `cargo test --release --test multi_workspace_migration`: PASS, 16 tests.
- `cargo test --release --test sync_local`: PASS, 7 tests with local rclone.
- `cargo test --release --test watch_local`: PASS, 2 tests.
- `cargo test --release --test status_read_only`: PASS, 15 tests.
- `cargo test --release --test receiver_setup_security`: PASS, 4 tests.
- `cargo test --release --test opencode_smoke`: PASS, 7 tests.
- `cargo test --release --test workspace_docs`: PASS, 27 tests.
- Root/workspace/migrate/sync-setup/receiver/server help audit: PASS.
- Long and short conflicting frontend flags: PASS, exact exit 2 refusal.
- Temporary two-workspace CLI smoke: PASS for workspace list, selected config
  and env list, family sync/receiver status, server status, and OpenCode
  fail-fast. The isolated tree was moved to Trash.
- Stale global paths, obsolete commands, hard-coded roots, frontend branches,
  overclaimed security, unfinished markers, and new rhetorical em dashes: PASS.
- Final base-to-HEAD added-line audit for rhetorical em dashes: PASS with zero
  hits after the scoped prose corrections.
- `git diff --check`: PASS.

## Rustfmt audit and deferred Minor

The exact starting commit already failed repo-wide `cargo fmt --check` with
1,310 formatter diff lines and 100 diff headers. Before Task 8 focused
formatting, the current branch had 1,121 lines and 87 headers. Rustfmt ran with
edition 2024 and `skip_children=true` over only the 102 Phase 5/Task 8 Rust
files. It changed the Task 8 docs test and five Phase 5-touched sync files.

Focused rustfmt check passes. The final repo-wide audit still reports 1,003
lines and 76 headers across 23 path-normalized files, but comparison with the
exact Phase 5 base reports zero current-only paths. This inherited formatting
drift remains the only deferred Minor; retaining a large unrelated formatting
sweep was intentionally rejected.

## Boundary

No production remote, real provider credential, live TUI, or real agent PTY was
used. The local-rclone and composed acceptance suites use temporary data and
fake only external provider/agent edges. No push, publication, merge, or
worktree deletion occurred.

## Final review correctness pass

The final independent review found six correctness defects and one deferred
Minor after the original Task 8 commit. This consolidated pass fixes all six
actionable findings and bumps the compatible release to `0.35.1`.

### RED evidence

1. `migration::users::tests::mapping_preflight_prefers_canonical_assigned_to_over_legacy_assignee` failed with `legacy-person` selected instead of `canonical-person`.
2. `sync_repair_and_check_refuse_mismatched_remote_before_any_data_command` failed because compiled sync created its UUID bisync workdir before remote identity refusal. `sync::args::tests::excludes_task_schema_metadata_for_schema_last_publication` failed because `tasks/SCHEMA.json` remained in rclone argv.
3. `setup_holds_the_uuid_lock_against_manual_sync_through_the_baseline` initially could not express a shared setup lock boundary. `concurrent_empty_setup_elects_one_claim_without_overwriting_the_manifest` then observed two canonical manifest publications.
4. `configured_legacy_plan_finishes_legacy_sync_before_uuid_cutover` lacked `PublishTaskSchemaTransition`; the schema-transition integration tests initially had no API to call.
5. The two `reload_after_sync` compiled tests failed substantively: migration accepted a sender mapping pulled by final sync, and a pulled disabled-triage config still produced managed triage rows.
6. The real-rclone `legacy_sync_migrates_then_syncs_and_a_second_legacy_machine_joins` test reached current-schema convergence but failed byte equality because independent migration placed `task_id` in different header positions.
7. Crash-window follow-up tests proved ordinary sync reached rclone and setup reached identity while the migration journal remained active. A final argv test proved setup ownership claims were not excluded from generic transport.

### Implemented boundaries

- Canonical `assigned_to` wins over legacy `assignee` regardless of column order.
- Remote identity succeeds before bisync workdir creation or stale-lock reaping.
- Setup holds one UUID sync lock across remote ownership, credential persistence,
  marker bootstrap, and initial baseline. Concurrent empty setup uses append-only
  exact-manifest claims, strict read-back/list validation, deterministic UUID
  election, canonical re-probe, and immutable-copy defense. Claims remain remote
  setup metadata and are excluded from ordinary transfer.
- The migration journal includes a remote schema-transition step. Local CSV
  migration and remote transition share the UUID lock; current task and habit
  CSVs publish first, both exact machine-local baselines are made durable next,
  and `tasks/SCHEMA.json` publishes last. Generic rclone excludes all three.
- Ordinary sync and setup refuse while an active rollout journal exists, so a
  process crash cannot reopen the legacy/current boundary before resume.
- Config, users, and both assignment CSVs are reloaded and preflighted
  immediately after the final legacy sync and before backup or portable
  mutation.
- Migrated CSV headers always begin `task_uuid,task_id`, giving independent
  machines a common byte representation.

### Focused verification

- `cargo test --release --test multi_workspace_migration -- --nocapture`: PASS, 20 tests.
- `cargo test --release --test sync_local -- --nocapture`: PASS, 8 tests with real local rclone.
- `cargo test --release --test sync_workspace_identity -- --nocapture`: PASS, 9 tests.
- `cargo test --release --test task_schema_migration -- --nocapture`: PASS, 10 tests.
- `cargo test --release --lib sync::setup::tests -- --nocapture`: PASS, 12 tests.

### Final verification

- `cargo test --release`: PASS, 1,306 library tests plus every integration and
  doc test.
- `cargo clippy --release --all-targets -- -D warnings`: PASS. The first strict
  run exposed only mechanical warnings in new test code; after correction, the
  affected tests and the final strict run remained green.
- `cargo test --release bundled_skills_carry_no_personal_data`: PASS.
- `python3 -m unittest discover -s skills/todo/scripts/tests`: PASS, 23 tests.
- Explicit acceptance, migration, local-rclone, watcher, literal read-only,
  receiver-security, OpenCode, and workspace-doc integration suites: PASS.
- Root/workspace/migrate/sync-setup/receiver/server help and both long and short
  conflicting-frontend audits: PASS.
- Isolated two-workspace CLI smoke: PASS. Its temporary tree was moved to Trash.
- Focused rustfmt over every touched Rust file, `git diff --check`, and
  added-line security/path/unfinished-marker/em-dash audits: PASS.
- No production remote, real provider credential, live TUI, or real agent PTY
  was used.

### Deferred Minor

Existing backup-path validation resolves canonical and lexical symlink aliases,
but a symlink inserted into a previously missing descendant after validation
can still race component creation. Descriptor-relative path walking is a
separate hardening design and is deferred; this pass does not expand the backup
transaction beyond the six actionable review findings.

## Consolidated final-review wave

A subsequent consolidated review found eight additional correctness boundaries:
late remote-claim arrival, migration activation before journal creation, remote
task-schema compatibility, setup from an already-current unconfigured machine,
complete canonical headers, transition-aware recovery, stale-lock takeover, and
clean-only credential persistence. Red tests were observed for every production
boundary before the corresponding fix. The bounded pre-existing backup-base
symlink issue was also reproduced and fixed; descriptor-relative insertion-race
hardening remains the sole deferred backup item.

Focused verification is green: setup 15/15, lock 6/6, recovery 2/2,
header/schema transform 2/2, remote-schema cases 3/3, real-rclone schema
transitions 2/2, and strict Clippy. The compatible patch version is `0.35.2`.
The final release suite passed with 1,321 library tests plus every integration
and doc test. The exact real-rclone identity rerun passed 9/9. Strict Clippy,
the personal-data guard, 23 Python skill tests, every explicit acceptance and
security suite, CLI help/conflict checks, the isolated two-workspace smoke,
focused rustfmt, and patch-hygiene audits all passed. Exact counts and commands
are recorded in the SDD progress ledger.

## Final closure wave

The last review wave closes four release blockers. A configured legacy machine
can now join an already-current remote through a migration-owned, replayable,
local-only task-id bridge that preserves remote UUID authority and never
publishes a legacy generation. Present remote schema metadata is strict and
complete; only true absence means legacy. Every active-journal failure reports
resume-only recovery because remote publication can precede its durable step
record. Backup inventory validation rejects pre-existing nested symlinks and
non-directory components before copying.

RED evidence was observed for the missing strict remote parser, restore-capable
recovery, nested backup symlink acceptance, the real coordinator schema
mismatch, the missing join seam, and incomplete current-manifest acceptance.
Focused GREEN verification covers CSV sync 18/18, setup 15/15, migration 22/22,
real-rclone sync 11/11, recovery 2/2, nested backup safety, strict remote
schema refusal without publication, replay identity, and strict Clippy. The
compatible patch version is `0.35.3`; full release gates are recorded in the
SDD progress ledger.

The only descriptor-relative post-validation insertion TOCTOU remains deferred
Minor.

Final release verification passed with 1,325 library tests plus every
integration and doc test. Strict Clippy passed at 0.35.3; the real-rclone suite
passed 11/11, migration passed 22/22, setup passed 15/15, stale-lock passed 6/6,
and remote identity passed 9/9. The bundled-skill personal-data guard, all 23
todo-skill Python tests, focused rustfmt, patch whitespace, exact version, and
added-line rhetorical-em-dash audits also passed.

## Counter closure wave

The remaining counter collision was reproduced at both the replayable bridge
and real coordinator boundaries. Generic bisync excludes the task and habit
counter files, the current-remote join path defers ordinary task-state
reconciliation, and the bridge previously wrote only CSVs. After Machine A
published real T7 and H8 allocations, Machine B completed the join with both
counters still at 2; the next real task or habit allocator would therefore
reissue an existing display ID. The exact RED commands and failures are
recorded in the SDD progress ledger before production changes.

The bridge now uses the existing counter parser and max semantics, adds the
joined display floor through one pure helper, fetches both remote counters,
and atomically replaces only local counter files before the journaled step can
complete. Missing and malformed counter inputs fall back to joined rows. Two
bridge replays are stable and have no remote publication path. The real
coordinator regression then proves the next actual task and habit allocations
are T8 and H9 rather than collisions. Focused migration, real-rclone,
counter, mutator, formatting, and strict-Clippy verification is green at
`0.35.4`; full release gates are recorded next in the SDD progress ledger.

Final release verification passed with 1,326 library tests plus every
integration and doc test. The explicit acceptance, migration, real-rclone,
identity, setup, lock, watcher, read-only, receiver-security, OpenCode, and
workspace-doc suites all passed at their recorded counts. The privacy guard,
all 23 Python skill tests, every requested CLI help/conflict form, focused
rustfmt, patch whitespace, intended-path, exact-version, unfinished-marker,
and added-line rhetorical-em-dash audits also passed. The compatible patch
version is `0.35.4`.
