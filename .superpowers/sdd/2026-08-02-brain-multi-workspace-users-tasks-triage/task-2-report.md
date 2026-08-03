# Task 2 report: effective actor resolution and preservation

## Status

Complete on `feat/workspace-users`. The branch is ready for the parent agent to
review and integrate. It has not been pushed or merged.

## Delivered

- Added a private-field, immutable `ActorContext` with typed person ID, display
  name, and initiating channel. Follow-up contexts retain the initiating actor.
- Added pure local, SMS, and email actor resolution. Authenticated inbound
  sender identity overrides the machine-local user; unknown or disabled senders
  are rejected.
- Kept provider authentication ahead of portable sender resolution. Queued
  receiver jobs now carry the workspace UUID and resolved `ActorContext`, so an
  untrusted sender string never becomes `BRAIN_ACTOR_ID`.
- Resolved the interactive actor once at TUI startup. Agent launches, follow-up
  turns, triage launches, direct reindex script environments, completion hooks,
  and delivery use the actor from the job or session context instead of
  re-reading a machine default.
- Scoped warm receiver-panel reuse and resumable sessions by frontend,
  workspace, actor, and channel. A different initiating actor cannot inherit an
  existing panel or conversation.
- Generalized the state schema to `agent_kind`, `agent_session_id`,
  `workspace_id`, `actor_id`, and `channel`. The v3 migration preserves old rows
  as the selected local user's interactive Claude sessions, including locks,
  source, and recency.
- Extended the same lifecycle to Claude and Codex. Codex completion artifacts
  use a stable response ID while retaining the actual thread ID; both hooks
  carry and verify actor/channel attribution.
- Restricted email responses to enabled identities owned by the initiating
  actor. Completion artifacts with mismatched actor or channel are discarded.
- Preserved Phase 5 boundaries: no owner, creator, audit, authorization, device,
  or account semantics were added, and no inactive legacy-user migration helper
  was activated.
- Updated architecture, glossary, features, data model, configuration,
  integrations, decisions, and testing documentation. Bumped Brain from
  `0.18.3` to `0.19.0`.

## TDD evidence

- `tests/actor_resolution.rs` first failed with unresolved `brain::actor`; all
  six precedence, rejection, and follow-up tests passed after implementation.
- `tests/state_actor_migration.rs` first failed because the scoped state API and
  identity-aware migration entry point did not exist; both migration and scope
  tests then passed.
- Hook integration initially failed against the generalized state schema; the
  session-start hook update restored all nine hook tests.
- Session environment tests first failed at the old function arity, then passed
  after typed actor, frontend, and stable response attribution were threaded
  through both launch paths.
- Receiver-security tests first failed because authenticated actor resolution
  did not exist; all eight passed after provider-first resolution was added.
- Receiver HTTP tests exposed fixtures with a fake home and no portable users;
  real temporary workspace/user fixtures fixed the test boundary, and all seven
  passed.
- Delivery tests first failed because actor-scoped recipient selection did not
  exist; all three passed after its implementation.
- The Stop-hook Codex thread/response-ID case has regression coverage in
  `tests/stop_hook_actor.rs`.

## Verification

- Focused actor/state/start-hook/stop-hook integrations: 18 passed.
- Focused receiver security: 8 passed; receiver HTTP: 7 passed; delivery: 3
  passed; completion actor/channel matcher: 1 passed.
- `cargo test --release bundled_skills_carry_no_personal_data`: passed.
- `cargo test --release` with Python 3.14 first in `PATH`: 1,138 tests passed,
  including all bundled Python-script integration tests.
- `cargo clippy --release --all-targets -- -D warnings`: passed.
- Explicit `rustfmt` over every changed Rust file: passed.
- `git diff --check`: passed.

The first full-suite attempt inherited macOS `/usr/bin/python3` 3.9 and failed
two unchanged bundled-script tests on existing Python 3.10 `date | None`
syntax. Repeating with `/opt/homebrew/bin` first in `PATH` selected Python 3.14
and passed the entire suite.

## Known repository-level verification issue

`cargo fmt --all -- --check` under the installed Rust 1.95 formatter reports
extensive pre-existing formatting drift in untouched files. No Task 2 file is
among the reported paths, and every changed Task 2 Rust file was formatted with
that formatter.

## Files changed

- `Cargo.toml`, `Cargo.lock`
- `src/actor/{mod,context,resolve}.rs`, `src/lib.rs`
- `src/workspace/context.rs`, `src/session.rs`, `src/state.rs`
- `src/server/{security,receiver,delivery,reply}.rs`
- `src/server/receiver/http/{mod,sms,email}.rs`
- `src/tui/{mod,app_brain,app_triage_tab}.rs`
- `src/tui/app_state/construct.rs`, `src/tui/event_loop/setup.rs`
- `src/reindex/tasks.rs`
- `scripts/claude_{session_start,stop}_hook.py`
- `tests/{actor_resolution,state_actor_migration,stop_hook_actor}.rs`
- `tests/{hook_integration,state_concurrency}.rs`
- `tests/workspace_runtime_isolation/support.rs`
- `docs/{architecture,config,data-model,decisions,features,glossary,integrations,testing}.md`

Original Task 2 commit: `c72ef10`; no push or merge was performed.

## Fix round 1 of 5

### Status

Complete. This round closes the legacy-readiness crash, binds one actor at the
ordinary command boundary, scopes opaque session identity consistently, and
refreshes current Claude and Codex lifecycle hooks before TUI state migration
or agent launch. No Task 3 assignment behavior was added.

### Root causes and corrections

- Readiness deliberately accepted a legacy workspace with a non-empty local ID
  and no `users.json`, but `local_actor` later required the missing file. Such a
  workspace now receives an immutable interactive compatibility actor without
  writing portable user data.
- `CommandContext` previously carried only the workspace. It now resolves and
  pins one actor during ordinary bootstrap. Task completion, habit mutation,
  reindex children, TUI state, and agent launches receive that same actor.
- State schema v3 treated the frontend's opaque session ID as globally unique.
  Schema v4 uses the composite key `(agent_kind, agent_session_id,
  workspace_id, actor_id, channel)`. Hook upsert, claim, and dead-lock reaping
  all address the same complete scope.
- Hook deployment happened during receiver setup only. Every TUI now refreshes
  the selected workspace's Claude scripts/settings and the machine's Codex
  hooks before opening the state DB. Basename-matched stale registrations are
  removed before canonical project-relative entries are installed.
- Documentation still counted four common integration variables. It now names
  all five workspace/actor variables and separately layers agent kind.

### RED and GREEN evidence

- `cargo test --release --test actor_resolution
  legacy_workspace_without_portable_users_resolves_its_local_actor` failed
  reading missing `.config/users.json`; it passed after compatibility actor
  resolution was added.
- The bootstrap request-lifetime test failed to compile with `E0609` because
  `CommandContext` had no `actor`; it passed after boundary binding.
- The reindex child test failed with `E0061` because `run_py` accepted no actor;
  it passed after actor-aware environment propagation.
- The task completion test failed on the missing actor-aware mutation seam; it
  passed after the command and TUI paths passed the boundary actor.
- The equal-opaque-ID migration test failed on the v3 unique constraint. The
  scoped claim test then failed with `E0061`, and scoped reaping initially
  unlocked the live row. All five state migration/scope tests now pass.
- The conflicting-attribution hook test initially returned no preserved rows
  under the composite schema. It passed after scoped upsert and rotation.
- The Codex lifecycle test first failed because automatic installation was not
  injectable. After lifecycle support existed, the strengthened stale-command
  case failed by invoking `/old/claude_session_start_hook.py`; it now passes
  through the actual installed Codex `SessionStart` and `Stop` commands.
- The first final full-suite run exposed six unit fixtures that resolved actors
  through intentionally fake `/home/tester` paths. Explicit immutable test
  actors corrected the fixture boundary; the production resolver stayed at
  ordinary bootstrap.

### Final verification

- Focused actor resolution: 7 passed.
- Focused state migration and scope isolation: 5 passed.
- Real start-hook integration: 10 passed; real Codex configured lifecycle: 1
  passed; Stop-hook actor contract: 1 passed.
- Focused bootstrap, task mutation, and reindex actor seams: 3 passed.
- `cargo test --release bundled_skills_carry_no_personal_data`: passed.
- `PATH="/opt/homebrew/bin:$PATH" cargo test --release --quiet`: 1,145 tests
  passed.
- `cargo clippy --release --all-targets -- -D warnings`: passed.
- `rustfmt --edition 2024 --config skip_children=true --check` over every
  changed Rust file: passed.
- `git diff --check`: passed.
- No bundled Python test directory exists; the full Rust integration suite
  executes the bundled hook and task scripts.

Repository-wide `cargo fmt --all -- --check` still reports the documented,
pre-existing Rust 1.95 formatting drift in untouched files. No changed file
fails the scoped formatter check.

### Self-review

The request actor is resolved once and reused across the delivered local
seams. Legacy compatibility is read-only. Equal opaque IDs cannot overwrite,
claim, or reap another immutable scope. Hook refresh precedes v3 migration and
both frontend launch paths are exercised. Task mutation accepts actor context,
but intentionally does not read or write `assigned_to`; that remains Task 3.
The patch version is `0.19.1`. The fix-round commit is created immediately
after this report; no push or merge is performed.
