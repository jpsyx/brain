---
id: BR-10
title: Recover stuck receiver turns with transcript-aware polling
status: cancelled
priority: none
assignee: jpsyx
labels: [enhancement, server]
estimate:
project: PROJ-1
milestone: MS-3
cycle:
parent:
github:
blocked_by: []
created: 2026-08-13
updated: 2026-08-23
---

# BR-10: Recover stuck receiver turns with transcript-aware polling

## Description

Replace the receiver's coarse five-minute inactivity drain with a regular,
frontend-neutral progress poll that can distinguish a queued prompt from a
prompt that was actually submitted. Inspect the active chat transcript or the
equivalent lifecycle evidence for Claude, Codex, and OpenCode to determine
whether the dispatched message entered the conversation and whether the turn is
still making progress.

This proposal was superseded by PROJ-1. Receiver injection and warm-panel reuse
will be removed entirely. BR-15 retains the exact prompt-acceptance and progress
evidence; BR-16 retains the bounded recovery and queue-advancement behavior.

## Acceptance criteria

- [x] Requirements preserved in BR-15 and BR-16.
- [x] Original task cancelled rather than implemented against the injection
      architecture it was meant to rescue.

## Notes

### Pointers (as of 2026-08-23)

- `docs/product-manager/projects/PROJ-1-rearchitect-receiver-processing.md`
  records the replacement architecture and ordered implementation work.
- `docs/product-manager/tasks/BR-15-prove-receiver-job-acceptance-and-progress.md`
  owns exact-token submission and progress evidence.
- `docs/product-manager/tasks/BR-16-recover-stalled-receiver-jobs.md` owns
  bounded same-session recovery, sender notification, restart reconciliation,
  and queue advancement.

### Log

- 2026-08-13 created from a proposal to replace the five-minute inactivity
  drain with transcript-aware submission and progress polling.
- 2026-08-15 marked likely superseded by BR-12's per-message process model.
- 2026-08-23 cancelled and archived. Its durable requirements were split into
  BR-15 and BR-16 under PROJ-1.
