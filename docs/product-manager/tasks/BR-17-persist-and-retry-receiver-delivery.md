---
id: BR-17
title: Persist and retry receiver response delivery separately
status: backlog
priority: high
assignee: jpsyx
labels: [enhancement, server]
estimate: 5
project: PROJ-1
milestone: MS-3
cycle:
parent:
github:
blocked_by: [BR-15]
created: 2026-08-23
updated: 2026-08-23
---

# BR-17: Persist and retry receiver response delivery separately

## Description

Persist the completed agent answer and transcript update before attempting an
SMS or email response. Provider delivery is a separate retryable phase whose
failure never reruns the agent. Restart reconciliation must resume delivery
from the recorded answer and preserve the job's acceptance-time sender,
recipient, subject, lineage, and authorization context.

## Acceptance criteria

- [ ] A token-matched completion atomically records answer readiness and the
      conversation transcript before provider delivery begins.
- [ ] SMS and email delivery use the job's immutable acceptance-time response
      identity and cannot widen recipients after later config changes.
- [ ] Provider failure records retry state and never returns the job to agent
      processing.
- [ ] Restart during answer recording, delivery, provider acknowledgement, or
      final state commit reconciles without losing an answer or deliberately
      issuing duplicate agent work.
- [ ] Delivery retries are bounded and idempotent where provider identifiers
      permit; ambiguous acknowledgement is recorded and surfaced explicitly.
- [ ] A terminal delivery failure notifies through any remaining safe channel
      when possible, records diagnostics, and allows later jobs to proceed.
- [ ] Email subject/message lineage and allowed thread participants remain
      intact; SMS formatting and length behavior remain unchanged.
- [ ] Red/green tests cover response persistence, provider retry, crash points,
      authorization changes, duplicate acknowledgements, and terminal failure.
- [ ] Response, transcript, integration, architecture, data-model, feature,
      decision, and testing docs describe the phase boundary.

## Notes

### Pointers (as of 2026-08-23)

- `src/tui/app_brain/receiver/completion.rs` currently reads a response file,
  starts sync, sends the provider reply, and advances runtime state in one
  process-local path. Split these facts at durable transaction boundaries.
- `src/server/delivery/` owns background Twilio and Resend calls and their
  immutable reply shapes. Add persisted retry orchestration without moving
  provider credentials into durable job data.
- `src/tui/app_brain/receiver/email_reply.rs` enforces the trusted recipient
  intersection and reply lineage. Keep one semantic email delivery seam.
- `scripts/agent_session_stop_hook.py` and the OpenCode lifecycle plugin publish
  completion artifacts. Bind each completion to the exact job token before
  recording an answer.

### Log

- 2026-08-23 created from PROJ-1 planning.
