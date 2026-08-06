# Task 8 report: durable docs, release surfaces, and final verification

## Status

PASS. All required durable docs and release surfaces describe the completed
Phase 5 module tree and CLI. The final additive version is `0.35.0`. All release,
acceptance, privacy, read-only, lint, skill, help, and smoke gates passed. The
branch remains local and the worktree is preserved.

## Version and base

- Starting version: `0.34.1`
- Final version: `0.35.0`
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
