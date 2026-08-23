---
id: BR-18
title: Remove receiver injection and complete the cutover
status: backlog
priority: high
assignee: jpsyx
labels: [tech-debt, server]
estimate: 8
project: PROJ-1
milestone: MS-4
cycle:
parent:
github:
blocked_by: [BR-13, BR-14, BR-15, BR-16, BR-17]
created: 2026-08-23
updated: 2026-08-23
---

# BR-18: Remove receiver injection and complete the cutover

## Description

Make the durable per-message path the only receiver execution path. Remove
warm-panel receiver reuse, receiver text injection, main-panel takeover,
process-local queue authority, coarse screen-based abandonment, and state that
exists only to support those behaviors. Preserve generic interactive
`AgentController` operations that still have non-receiver callers.

Complete automatic migration, restart reconciliation, diagnostics, user-facing
status, documentation, and full verification. The finished system must expose
whether a job is queued, running, recovering, awaiting delivery, failed, or
done without leaking private message contents.

## Acceptance criteria

- [ ] No receiver code injects text or submit keys into an existing Claude,
      Codex, OpenCode, or main-panel process.
- [ ] The in-memory `VecDeque` and warm receiver lease are no longer queue or
      conversation authority.
- [ ] Legacy receiver state migrates automatically with tested `up` and `down`
      operations; help and version remain side-effect free.
- [ ] Receiver status and logs report durable queue depth, active job phase,
      recovery attempt, and delivery state using themed, redacted output.
- [ ] Shutdown and restart at each lifecycle phase leave jobs recoverable and
      never block later jobs indefinitely.
- [ ] Existing receiver routing, authorization, control commands, attachments,
      sync freshness, completion push, task reload, and response behavior remain
      supported or are deliberately redefined in docs and tests.
- [ ] The obsolete BR-10 watchdog and old receiver dispatch/completion tests are
      removed or rewritten around the durable lifecycle rather than retained as
      a dormant parallel path.
- [ ] All applicable files under `docs/` are updated in the same change,
      including architecture, features, integrations, decisions, data model,
      glossary, keybindings, config, and testing where relevant.
- [ ] `cargo test --release` passes.
- [ ] `cargo clippy --release --all-targets -- -D warnings` passes.

## Notes

### Pointers (as of 2026-08-23)

- `src/tui/app_brain/receiver/`, `src/tui/receiver/`, and
  `src/tui/app_actions/receiver.rs` contain the legacy execution, runtime,
  policy, and user-action seams. Remove only behavior superseded by the durable
  path and keep unrelated receiver controls intact.
- `src/server/receiver/` and `src/server/control/` own ingress, authority,
  routing, deduplication, and process lifecycle. Verify the cutover does not
  weaken their existing race and deadline guarantees.
- `src/tui/runtime/`, `src/tui/state/brain.rs`, and
  `src/tui/app_skill_session/` own startup/shutdown and tab lifecycle. Verify
  controller teardown, stale-claim recovery, and focus behavior together.
- `docs/README.md` and the AGENTS docs-contract table identify every durable
  product document affected by this server, receiver, session, and TUI change.

### Log

- 2026-08-23 created from PROJ-1 planning.
