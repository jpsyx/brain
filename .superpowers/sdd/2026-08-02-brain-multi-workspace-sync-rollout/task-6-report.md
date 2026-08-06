# Task 6 report: per-workspace requirements and status audit

## Status

PASS. Task 6 is implemented, documented, release-tested, lint-clean, and
committed locally in the single task commit. Nothing was pushed.

## Version and base

- Starting version: `0.33.0`
- Final version: `0.34.0`
- Starting commit: `67e3ac281b3da483c5a0ca5bd173c3539eaf3555`

## Outcome

Brain now has one selected-workspace requirements model that distinguishes
required availability from optional feature state. Required checks use
`ready` or `unavailable`; optional checks use `off`, `ready`, or `incomplete`.
Malformed active configuration no longer collapses to the disabled default.

The same redacted, themed model is consumed by `brain workspace list`,
`brain sync status`, `brain receiver status`, and `brain tasks doctor`.
Ordinary TUI readiness remains the owner of repair and startup gating, including
the existing markdown-to-PDF prerequisite. The requirements model reuses the
same required-field decision without replacing readiness.

## Implementation

### Central selected-workspace model

`src/workspace/requirements/` contains focused modules for the public model,
inspection, rendering, sync, receiver, optional features, and agent
capabilities. Every inspection reloads the exact canonical selected record and
verifies its UUID before reading feature state. It never falls back to a peer
workspace.

Each requirement carries:

- a typed scope;
- required or feature status;
- prompt metadata, including whether input is secret; and
- exact non-interactive remediation text.

Required health covers the selected root, compatible matching manifest,
nonempty portable users registry, and valid local-user membership. Optional
health covers cloud sync and its watcher, receiver and both channels,
workspace-only MCP and custom-skill capabilities, managed triage and its modal,
PDF conversion, Linear, personalization, and browser/web views.

### Honest active-feature inspection

- Sync is `off` only when absent, empty, or explicitly disabled. An enabled
  block must have correctly typed bucket, path, key ID, and application key to
  become `ready`; partial or malformed active data is `incomplete`.
- Receiver intent comes only from the existing persisted receiver flag. When
  intent is off, stale provider data is ignored. When intent is on, any provider
  field or portable inbound mapping activates that channel. Complete provider
  material, public URL, and a matching allowed portable identity are required
  for `ready`.
- Missing workspace-only MCPs and non-core skills are reported individually.
  Bundled core skills are not optional installation rows.
- `workspace_only` remains explicitly advisory and makes no isolation claim.
- Malformed Linear slugs are incomplete and never form navigable URLs.
- Malformed portable config is reported as incomplete and preserved.

No credential, token, sender address, phone number, email address, or stored
secret value is included in status output.

### Status and doctor integration

- `workspace list` keeps the sorted registry summary and appends the selected
  requirements matrix. It does not seed missing access modes or repair setup.
- `sync status` renders cloud-sync and watcher health before local run state.
  Data-moving setup, sync, repair, and check operations still validate the
  remote manifest UUID at their existing transport boundaries.
- `receiver status` preserves its four lifecycle rows and adds receiver, SMS,
  and email health from the selected workspace.
- `tasks doctor` groups state database, Claude SessionStart, Codex SessionStart,
  tools, and feature health under the selected workspace. OpenCode remains
  inert. Doctor success requires both active frontends.
- The doctor rclone probe uses an explicit no-config path, so a missing rclone
  config is never created during observation.

### Literal no-write status

Workspace list, sync status, receiver status, and tasks doctor use a dedicated
read-only selected-workspace bootstrap. It skips migration, readiness repair,
access-mode seeding, skill rendering, locks, logs, and state initialization.
The bootstrap still enforces a detached child's `BRAIN_WORKSPACE_ID`, so moving
sync status to the read-only path does not weaken exact-workspace routing.

SQLite status readers use immutable read-only URIs. This is stronger than
ordinary SQLite read-only mode, which can checkpoint a WAL database and mutate
the main database merely by opening it.

`tests/status_read_only.rs` snapshots bytes, metadata, symlinks, sockets, and
their referents around the compiled status commands. Dedicated fixtures cover
pre-existing WAL-mode sync journal and state databases.

### Documentation and release

The durable architecture, feature, configuration, data model, integration,
decision, and testing documentation was updated in the same change. The crate
version and lockfile moved from `0.33.0` to `0.34.0` for the additive
user-visible status feature.

## RED and GREEN evidence

The implementation followed the repository's red/green loop. Observed REDs
included:

- The initial requirements test failed to compile because the model and API did
  not exist.
- Required root, manifest, users, and local-user variants failed before their
  typed statuses were implemented.
- Sync tests first exposed missing raw inspection, then proved partial active
  configuration had been collapsed to disabled.
- Receiver tests first exposed missing channel health; a later RED showed stale
  fields made a disabled receiver incomplete, and another showed a malformed
  present field was incorrectly treated as off.
- Optional-feature tests initially failed on missing scopes and inspectors.
- Selected-record isolation failed before the inspector reloaded and
  UUID-checked the exact canonical record.
- Workspace-list, sync-status, receiver-status, and doctor compiled-process
  tests failed before the matrix was routed into each command.
- Read-only snapshots initially caught status-created state, rclone config, and
  status-path writes.
- Claude/Codex doctor parity initially failed to compile before the second
  frontend path and health state existed.
- Malformed Linear configuration initially produced a navigable path.

Every focused RED was followed by the smallest implementation and a focused
GREEN before the next behavior was added.

## Self-review findings fixed

1. **Bundled core skill misclassification.** The first capability inspector
   emitted optional setup rows for bundled core skills. A focused RED proved the
   incorrect rows, then both the valid-plan and malformed-plan branches were
   changed to exclude every bundled skill. This is a fixed self-review finding,
   not deferred work.
2. **SQLite WAL mutation through a read-only open.** Filesystem snapshots showed
   that ordinary SQLite read-only connections could checkpoint WAL state. Sync
   journal status and doctor now use escaped immutable SQLite URIs, with focused
   WAL regression tests for both databases.
3. **Detached sync child identity.** The first full release suite showed that
   moving `sync status` to read-only bootstrap bypassed the existing expected
   UUID validator. The read-only bootstrap now calls that same validator. The
   exact integration regression and full suite are green.
4. **Workspace-list mutation assumptions.** The full suite found two tests that
   still expected list to seed missing access modes. They now assert the Task 6
   no-write contract; malformed mode is rendered as incomplete without changing
   its bytes.
5. **Legacy sync compatibility.** A temporary strict unknown-field experiment
   would have rejected intentionally tolerated legacy sync keys. It was removed
   during self-review; malformed known fields still fail closed.
6. **Modularity.** The requirements implementation and its integration suite
   were split by responsibility. Every new production requirements file is
   below 200 lines, and no catch-all test fixture was introduced.

Command-filter mistakes that selected zero tests were not counted as behavioral
REDs. They were immediately rerun with matching filters.

## Final verification

- `cargo test --release`: PASS
- `cargo clippy --release --all-targets -- -D warnings`: PASS
- `git diff --check`: PASS

## Known boundary

The read-only requirements matrix reports local sync configuration health and
does not contact the remote. Exact remote workspace UUID matching remains
mandatory at every setup, sync, repair, and check boundary immediately before
data transport. This avoids network side effects and latency in general status
commands while preserving the data-moving safety invariant.

The doctor can detect both Claude and Codex SessionStart hooks. Its current
installer remediation is the existing workspace hook installer, which repairs
Claude configuration; Codex hook installation remains a separate machine-level
configuration concern.
