---
id: BR-13
title: Durably enqueue receiver jobs at ingress
status: backlog
priority: high
assignee: jpsyx
labels: [feature, server]
estimate: 5
project: PROJ-1
milestone: MS-1
cycle:
parent:
github:
blocked_by: [BR-12]
created: 2026-08-23
updated: 2026-08-23
---

# BR-13: Durably enqueue receiver jobs at ingress

## Description

Change authenticated SMS and email ingress so provider success is returned only
after the exact job is durably committed. The shared server remains an ingress
and routing process, not an agent executor, but it writes the workspace queue
instead of treating a live TUI socket append as durable acceptance.

Preserve the current routing, authority, signature, actor, recipient,
attachment, deadline, and provider-deduplication boundaries. A TUI may disappear
immediately after the provider receives success without losing the accepted
job.

## Acceptance criteria

- [ ] The authenticated pipeline persists the complete immutable job before
      returning provider success.
- [ ] A crash after durable commit but before or after the provider response
      cannot lose the job or create a duplicate on provider retry.
- [ ] Provider delivery IDs preserve current SMS/email deduplication behavior
      across process restarts.
- [ ] Final workspace authority and receiver enablement are revalidated before
      durable admission commits.
- [ ] An unavailable or disabled workspace retains the existing explicit
      provider-facing behavior and does not enqueue work.
- [ ] The shared server never launches an agent or owns conversation execution.
- [ ] Red/green tests cover commit-before-ack, response loss, provider retry,
      revocation, capacity/backpressure, and restart recovery without sleeps.
- [ ] Server, integration, feature, architecture, data-model, decision, and
      testing docs reflect the new acceptance boundary.

## Notes

### Pointers (as of 2026-08-23)

- `src/server/receiver/dispatch/` owns the ordered authenticated pipeline,
  final authority revalidation, provider deduplication, and live handoff. Move
  the acceptance commit without reordering its security-sensitive stages.
- `src/server/receiver/transport.rs` and `src/tui/singleton.rs` implement the
  current socket prepare/accept transaction. Retire queue authority here only
  after the durable transaction provides equivalent acknowledgement semantics.
- `src/server/receiver/admission.rs` linearizes receiver admission against
  enablement and lease revocation. Keep that race guarantee at the new commit
  boundary.
- `src/server/request.rs` and `src/server/receiver/http/` map provider outcomes
  and body limits. Provider success must continue to mean one accepted job.

### Log

- 2026-08-23 created from PROJ-1 planning.
