---
id: BR-10
title: Recover stuck receiver turns with transcript-aware polling
status: backlog
priority: none
assignee: jpsyx
labels: [enhancement, server]
estimate:
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-08-13
updated: 2026-08-13
---

# BR-10: Recover stuck receiver turns with transcript-aware polling

## Description

Replace the receiver's coarse five-minute inactivity drain with a regular,
frontend-neutral progress poll that can distinguish a queued prompt from a
prompt that was actually submitted. Inspect the active chat transcript or the
equivalent lifecycle evidence for Claude, Codex, and OpenCode to determine
whether the dispatched message entered the conversation and whether the turn is
still making progress.

When the evidence shows that submission never happened, or that a submitted
turn is stuck and no longer progressing, end the failed session and restart a
fresh session so queued messages can continue. Preserve genuinely slow turns
that still show progress, avoid submitting a queued message twice, and retain
the existing sender notification when Brain cannot safely recover the original
message.

The design should explicitly answer which evidence is authoritative for each
frontend, how polling avoids re-reading an entire growing transcript on every
tick, and how Brain behaves when transcript state is missing, delayed, or
ambiguous.

## Acceptance criteria

- [ ] Brain polls bounded submission and progress evidence for an in-flight
      receiver turn instead of relying on a fixed five-minute inactivity drain
      as the primary recovery mechanism.
- [ ] The poll can tell whether the exact queued message was submitted, without
      mistaking an older transcript entry or unrelated local turn for it.
- [ ] A prompt that was not submitted is recovered safely, without duplicate
      submission or duplicate delivery.
- [ ] A submitted turn that makes no progress is ended and its session is
      restarted so later queued messages continue; a turn with observable
      progress is left running.
- [ ] Claude, Codex, and OpenCode use equivalent behavior through
      `AgentController`, with frontend-specific transcript conventions kept in
      their adapters or registry integration.
- [ ] Missing, malformed, delayed, or unavailable transcript evidence has a
      conservative bounded fallback and produces useful receiver diagnostics.
- [ ] Red/green tests cover never-submitted, progressing, stuck, ambiguous, and
      recovery/restart cases without fixed sleeps.
- [ ] Receiver behavior and the rationale are updated in the applicable docs,
      including the removal or redefinition of the five-minute watchdog.
- [ ] `cargo test --release` and
      `cargo clippy --release --all-targets -- -D warnings` pass.

## Notes

### Pointers (as of 2026-08-13)

- `src/tui/app_brain/receiver/completion.rs` and
  `src/tui/app_brain/receiver/diagnostics.rs` currently poll completion
  artifacts, sample the PTY panel, and abandon quiet turns. Start here to
  replace the timeout decision while keeping the event loop nonblocking.
- `src/tui/receiver_state.rs` contains the pure timeout and recovery decisions
  plus focused tests. Put new submission/progress classifications here, or in a
  similarly pure focused module, before wiring them into the app.
- `src/agent/` and `src/agent/registry.rs` own frontend-specific session,
  transcript, and lifecycle conventions behind `AgentController`. Extend that
  abstraction rather than branching on Claude, Codex, or OpenCode in the TUI.
- `src/tui/app_brain/tests/receiver.rs` and the adjacent frontend receiver tests
  exercise queue progress, stuck-turn abandonment, and session reuse. Use their
  injected clocks and fixtures for the new red/green recovery cases.
- `docs/integrations.md`, `docs/features.md`, `docs/architecture.md`, and
  `docs/decisions.md` describe receiver dispatch, completion, queue recovery,
  and the current inactivity watchdog; update the applicable contracts in the
  implementation change.

### Log

- 2026-08-13 created from a proposal to replace the five-minute inactivity
  drain with regular transcript-aware submission and progress polling, followed
  by session restart when recovery is necessary.
