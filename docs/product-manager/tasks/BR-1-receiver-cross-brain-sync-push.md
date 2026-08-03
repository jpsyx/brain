---
id: BR-1
title: Receiver accepts cross-brain "files changed" sync-push POSTs
status: backlog
priority: none
assignee: jpsyx
labels: [feature, sync, server]
estimate:
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-08-03
updated: 2026-08-03
---

# BR-1: Receiver accepts cross-brain "files changed" sync-push POSTs

## Description

Today each brain learns that another machine changed files only reactively:
the startup pull, the local `notify` filesystem watcher (which sees only
*local* edits), and the receiver **freshness gate** that pulls the remote at
most once every 2 hours before doing receiver work
(`MESSAGE_PULL_MAX_AGE` in `src/sync/freshness.rs`). A running brain can
therefore sit on stale data for up to 2 hours after another machine pushes.

Add a **push-notification path between brains**: when one brain finishes a
sync push, it POSTs a small "files changed, you should sync" signal to the
other brain(s)' receiver server, and the receiving brain reacts by kicking off
a pull immediately (via the existing detached-sync trigger). This makes
cross-machine propagation event-driven instead of poll-driven.

New receiver route (working name `POST /sync/notify`) added to the pure
router, authenticated like the other receiver endpoints, that on a valid
request fires a detached `brain sync --pull --if-idle` for the target
workspace. The `--if-idle` coalescing + the workspace-UUID lock already make
a redundant notification safe.

### Replace the 2-hour freshness gate (verify the logic first)

**Step (must be done as part of this task, before touching the gate): verify
this reasoning is actually correct against the current code.** The claim to
verify: because the receiver server lives inside the running TUI process
(`src/server/receiver.rs` — it is TUI-owned and dies with the process), *if
the TUI has been running continuously it must have received a sync-push POST
for every remote change during that window*. So the time-based
`message_pull_due` / `MESSAGE_PULL_MAX_AGE` freshness pull becomes redundant
and this push signal should **replace** it, not merely supplement it.

Confirm (or refute, and report back) each link in that chain before removing
the gate:

- The receiver only runs while the interactive brain process is up (so there
  is no window where the machine is "a brain that should sync" but has no
  listener). Check `ReceiverServer` lifetime and how/when it is started
  (`src/tui/receiver_state.rs`, `src/tui/event_loop/setup.rs`).
- Every sync **push** on the sending side reliably emits the notification to
  all peer brains (no silent drop when a peer is offline, no peer left out).
  Decide what the desired behavior is when a peer brain is **not** running
  (POST fails): is the next startup pull the intended safety net that lets us
  still drop the 2-hour timer? Document the answer.
- There is no other consumer of `message_pull_due` /
  `MESSAGE_PULL_MAX_AGE` that still needs the time-based fallback.

If the logic holds, remove the 2-hour freshness pull in favor of the push
signal. If it does **not** hold (e.g. offline peers would silently miss
updates with no fallback), do **not** remove it; instead report the gap and
propose keeping a (possibly longer) timer as a backstop, and let the user
decide.

## Acceptance criteria

- [ ] Verification step above completed and its conclusion recorded in this
      task's Notes (logic confirmed or refuted, with the offline-peer decision
      documented) **before** the freshness gate is changed.
- [ ] Pure router (`src/server/router.rs`) recognizes the new sync-notify
      `POST` route, with exhaustive unit tests (RED first) covering the happy
      path, wrong method, and unknown paths — matching the existing `Route`
      test style.
- [ ] A valid, authenticated sync-notify POST triggers a detached
      `brain sync --pull --if-idle` for the target workspace; an unauthenticated
      or malformed one is rejected with the same themed JSON error shape the
      other receiver endpoints use.
- [ ] The sending side (sync push completion) emits the notification to peer
      brains; behavior when a peer is offline is defined and tested.
- [ ] The 2-hour `message_pull_due` freshness pull is either removed (if the
      logic holds) or explicitly retained as a documented backstop (if it does
      not), per the verification outcome.
- [ ] Docs updated per the `docs/` contract (see Pointers): the auto-sync
      triggers row in `docs/features.md` + `docs/architecture.md` +
      `docs/integrations.md` + `docs/decisions.md`, and the server-endpoint row
      in `docs/architecture.md` + `docs/features.md` + `docs/integrations.md`.
- [ ] `cargo test --release` and `cargo clippy --release --all-targets` clean;
      crate version bumped in `Cargo.toml` + `Cargo.lock`.

## Notes

### Pointers (as of 2026-08-03)

High-level map of where this lives. Verify/refresh before starting — the code
moves.

- `src/server/router.rs` — pure `route(method, path) -> Route` mapper and its
  exhaustive unit tests. Add the new `Route::SyncNotify` variant and its route
  here first (RED test), same pattern as `Sms`/`Email`/`TriageDone`.
- `src/server/receiver/http/mod.rs` — the receiver worker's `respond()`
  dispatch on `Route`, body reading, auth-gated `enqueue`, and JSON/XML
  response helpers. Wire the new route's handler here; reuse `SecurityConfig`
  and the themed JSON error shape. New handler likely mirrors a small
  `receiver/http/<name>.rs` file next to `email.rs`/`sms.rs`.
- `src/server/receiver.rs` — `ReceiverServer` is TUI-owned and dies with the
  process; `DEFAULT_PORT = 8788`. Key evidence for the "TUI running ⇒ all
  POSTs received" verification claim.
- `src/server/security.rs` — how existing receiver endpoints authenticate;
  follow the same mechanism for the new route (a shared secret between peer
  brains is the likely approach — decide and document).
- `src/sync/freshness.rs` — `message_pull_due` + `MESSAGE_PULL_MAX_AGE`
  (the 2-hour gate this task replaces). Find all callers before removing.
- `src/sync/trigger.rs` — detached `brain sync … --if-idle` spawner; reuse
  `detached_sync_args(..., Direction::Pull)` to kick the pull on notify.
- `src/sync/{command,run}.rs` — sync push pipeline; the sending side must emit
  the peer notification when a push completes. Peer addresses/secret likely
  belong in the `sync` config block (`src/sync/config.rs`) and env
  (`src/env/`, `docs/config.md`).
- `src/tui/receiver_state.rs`, `src/tui/event_loop/setup.rs`,
  `src/tui/app_sync.rs` — how the receiver + auto-sync are started/wired into
  the TUI; needed for the lifetime part of the verification.
- `docs/integrations.md` + `docs/architecture.md` + `docs/features.md` +
  `docs/decisions.md` — the auto-sync-triggers and server-endpoint rows in the
  `docs/` contract that must be updated in the same change.

### Log

- 2026-08-03 created. Scope: add a cross-brain receiver sync-push route; and,
  after verifying the "TUI running ⇒ every remote change was POSTed" logic,
  replace the 2-hour freshness pull with the push signal (or keep it as a
  documented backstop if the logic doesn't hold).
