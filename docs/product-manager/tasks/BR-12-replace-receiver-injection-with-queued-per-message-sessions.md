---
id: BR-12
title: Replace receiver injection with a server queue and per-message ephemeral sessions
status: backlog
priority: high
assignee: jpsyx
labels: [enhancement, server, tech-debt]
estimate:
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-08-15
updated: 2026-08-15
---

# BR-12: Replace receiver injection with a server queue and per-message ephemeral sessions

## Description

Stop driving remote (SMS / email) work by injecting text into the already
running brain-panel agent process. Injecting into a live PTY session has been
the source of a large class of bugs (queued-but-never-submitted prompts, stuck
turns, waiting for the main session to go idle, view switching side effects).

New model:

- The TUI is for in-person use, but a TUI must still be running for remote
  messages to be handled at all. It is the host; it is no longer the thing we
  inject into.
- When the receiver takes in an SMS or email, the **server queues** that
  message. The queue is the single source of truth for pending remote work.
- Remote work runs in its **own tab** in the TUI (its own agent process, in the
  spirit of the existing skill-session tabs), never in the main brain panel. No
  view switching, no waiting for the main session to be idle, no shared process.
- The tab is started with the received message as a **pre-filled initial
  prompt**, runs it to completion, and then the tab closes immediately.
- Messages that arrive while a remote tab is running simply stay queued. On
  close, if the queue is non-empty, open a fresh tab with the next message as
  its initial prompt. Repeat until the queue drains.

Session continuity: we would like to resume the prior SMS/email conversation
with the prompt pre-filled. If any frontend cannot start a *resumed* session
with a pre-filled prompt, fall back to **Brain-owned transcript history**: keep
our own markdown transcript per remote channel/thread, always start a fresh
session, and have the initial prompt point at that markdown file so the agent
has the full prior context. Either way, we never reuse a running agent process.

This supersedes most of the recovery machinery in BR-10 (transcript-aware
polling to rescue a stuck injected turn) — with one process per message, a
failed run is bounded and simply retried or reported. Decide BR-10's fate
(close, narrow, or fold in) when this is picked up.

## Acceptance criteria

- [ ] Received SMS/email messages are queued in the server and survive until
      explicitly consumed.
- [ ] Remote messages are handled in a dedicated ephemeral TUI tab with a fresh
      agent process, never by injecting into the main brain panel.
- [ ] The tab starts with the message pre-filled as its initial prompt and
      closes itself on completion.
- [ ] A message arriving mid-run stays queued and is picked up by the next tab;
      the queue drains without manual intervention.
- [ ] No path waits for the main brain-panel session to be idle.
- [ ] Conversation continuity works for Claude, Codex, and OpenCode through
      `AgentController` — either by resuming with a pre-filled prompt or via the
      Brain-owned markdown transcript fallback.
- [ ] The old injection/dispatch path (and its inactivity watchdog) is removed,
      not left dormant alongside the new one.
- [ ] Red/green tests cover queue ordering, run-to-close lifecycle, arrival
      during a run, and the transcript fallback, without fixed sleeps.
- [ ] `docs/architecture.md`, `docs/features.md`, `docs/integrations.md`,
      `docs/decisions.md`, `docs/glossary.md`, and `docs/keybindings.md` updated
      as applicable.
- [ ] `cargo test --release` and
      `cargo clippy --release --all-targets -- -D warnings` pass.

## Notes

### Pointers (as of 2026-08-15)

- `src/tui/app_brain/receiver/` — the current dispatch/completion/diagnostics
  path that injects into the live panel and polls for quiet turns. This is what
  gets torn out and replaced by a queue consumer.
- `src/tui/receiver_state.rs` — pure receiver decision logic and its tests. The
  new queue/ordering/lifecycle decisions belong here (or in a sibling pure
  module), not in the event loop.
- `src/tui/app_skill_session/` — the existing model for an ephemeral,
  single-prompt tab that opens with a pre-filled prompt and closes itself on
  completion. This is the closest existing pattern; the remote tab should follow
  it rather than invent a second lifecycle.
- `src/skill_session/{prompt,signal,editor}.rs` — prompt construction and the
  completion signal protocol for those ephemeral sessions.
- `src/command/server/receiver/` and `src/server/routes/` — where incoming
  SMS/email lands and where the queue (and any session-done route) should live.
- `src/agent/registry.rs`, `src/agent/registry/contract.rs`, `src/session.rs` —
  per-frontend session construction; this is where "can this frontend resume
  with a pre-filled prompt?" is answered for Claude, Codex, and OpenCode, and
  where the transcript-fallback branch belongs.
- `docs/product-manager/tasks/BR-10-*.md` — largely superseded by this task;
  reconcile before starting.

### Log

- 2026-08-15 created. Motivation: text injection into running agent processes
  has been persistently buggy; replacing it with a server queue plus fresh
  per-message agent processes removes the shared-process and idle-wait failure
  modes entirely.
