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

Commit: created immediately after this report; no push or merge was performed.
