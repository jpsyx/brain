---
id: BR-13
title: Durably enqueue receiver jobs at ingress
status: done
priority: high
assignee: jpsyx
labels: [feature, server]
estimate: 5
project: PROJ-1
milestone: MS-1
cycle:
parent:
github:
blocked_by: []
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

- [x] The authenticated pipeline persists the complete immutable job before
      returning provider success.
- [x] A crash after durable commit but before or after the provider response
      cannot lose the job or create a duplicate on provider retry.
- [x] Provider delivery IDs preserve current SMS/email deduplication behavior
      across process restarts.
- [x] Final workspace authority and receiver enablement are revalidated before
      durable admission commits.
- [x] An unavailable or disabled workspace retains the existing explicit
      provider-facing behavior and does not enqueue work.
- [x] The shared server never launches an agent or owns conversation execution.
- [x] Red/green tests cover commit-before-ack, response loss, provider retry,
      revocation, capacity/backpressure, and restart recovery without sleeps.
- [x] Server, integration, feature, architecture, data-model, decision, and
      testing docs reflect the new acceptance boundary.

## Notes

### Pointers (as of 2026-08-23)

- `src/server/receiver/dispatch/pipeline.rs` owns the ordered authenticated
  pipeline and constructs the complete immutable `InboundJob`. Move its final
  acceptance boundary without reordering routing, authentication, actor, or
  authority decisions.
- `src/state/receiver/store.rs` owns schema-v6 transactional acceptance and
  provider deduplication. Extend that transaction with the durable queued
  capacity decision before ingress depends on it.
- `src/server/receiver/admission.rs` linearizes receiver admission against
  enablement and lease revocation. Keep that race guarantee around the durable
  commit without holding control authority during SQLite IO.
- `src/server/request.rs`, `src/server/receiver/http/`, and
  `src/server/receiver/dispatch/deliveries.rs` map provider outcomes and the
  current process-local deduplication behavior. Durable provider retries must
  resolve before capacity rejection while verified unavailable email remains a
  non-enqueued discard.

### Plan (2026-08-23)

1. Specify atomic durable capacity and deduplication ordering at the BR-12 store
   boundary, then implement the smallest transaction change.
2. Specify SMS and email conversation identity construction at ingress, using
   stable SMS identity and fresh email identity when Resend exposes no verified
   thread key.
3. Replace live socket acceptance with final-authority-guarded durable admission
   while preserving deadline, unavailable, revocation, and provider response
   semantics.
4. Cover commit-before-ack, response loss, provider retry, restart recovery,
   capacity, and revocation through focused and real-boundary tests with no
   timing sleeps.
5. Update the product contract, bump the additive pre-1.0 minor version, run
   formatting, release tests, Clippy, and hygiene checks, then commit BR-13.

### Log

- 2026-08-23 created from PROJ-1 planning.
- 2026-08-23 started after BR-12 established the schema-v6 durable receiver
  model.
- 2026-08-23 implemented authenticated SMS and email durable admission with
  atomic queued capacity, restart-safe provider deduplication, and ingress
  deadline enforcement before potentially blocking database setup.
- 2026-08-23 verified response-loss retry, abrupt shared-server crash recovery,
  revocation, unavailable routing, and the 64-job queued capacity through
  deterministic focused and composed HTTP tests. The release test and Clippy
  gates pass.
- 2026-08-23 corrected the verified-unavailable Email race by deferring a
  discard without releasing its in-flight provider reservation and returning
  503 until pending acceptance resolves. Receiver admission now also refreshes
  SQLite's busy timeout after database setup so sequential lock waits share the
  one absolute handoff deadline.
- 2026-08-23 serialized the receiver real-process fixtures, bounded the
  lifecycle scenarios, and made receiver child ownership unwind-safe, removing
  full-suite startup contention while keeping replacement fixture lifetimes
  explicit.
- 2026-08-23 completed in `f04f20a` and `efe5580`, then released as Brain
  0.73.2. Provider success now follows exact workspace-scoped durable admission;
  restart-safe deduplication, final authority, capacity, response-loss, and
  shared-process crash recovery passed the complete release suite.
