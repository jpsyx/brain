---
id: BR-12
title: Model durable receiver jobs and conversations
status: done
priority: high
assignee: jpsyx
labels: [enhancement, server, tech-debt]
estimate: 13
project: PROJ-1
milestone: MS-1
cycle:
parent:
github:
blocked_by: []
created: 2026-08-15
updated: 2026-08-23
---

# BR-12: Model durable receiver jobs and conversations

## Description

Replace the live TUI's in-memory queue as the source of truth with a durable,
workspace-scoped receiver model. The model must retain accepted jobs across
Brain and machine restarts, claim work without destructive pops, and represent
the processing, completion, delivery, retry, and terminal states needed by the
rest of PROJ-1.

Model logical conversations separately from jobs. A conversation belongs to a
workspace, portable user, channel, and conversation key. SMS uses one stable
conversation for the workspace-user-channel tuple. Email uses verified thread
lineage and starts a new conversation when lineage is unavailable or
ambiguous. Each conversation retains its current frontend and resumable native
session ID plus a continuously maintained Brain-owned transcript for recovery.

## Acceptance criteria

- [x] Accepted jobs survive orderly and crashed TUI or shared-server restarts.
- [x] A job remains durable while queued, claimed, launching, accepted,
      processing, answer-ready, delivering, retrying, failed, or done.
- [x] Claims name their owner and expire so a crashed consumer cannot strand a
      queue head.
- [x] Job and conversation identifiers are immutable and workspace-scoped;
      provider delivery IDs remain available for ingress deduplication.
- [x] There is no global Email session or SMS session. Conversation identity
      includes workspace, portable user, channel, and the channel-specific
      conversation key.
- [x] SMS maps to one stable conversation per workspace-user-channel tuple.
- [x] Email resolves verified thread lineage; uncertain lineage creates a new
      conversation and never merges from subject text alone.
- [x] A conversation stores the current frontend/native session binding and a
      portable Brain transcript. A frontend change starts fresh from that
      transcript rather than resuming another frontend's opaque session.
- [x] The schema has automatic `up` and `down` migrations and restart-safe
      reconciliation tests written red before production code.
- [x] Applicable data-model, architecture, integration, feature, decision, and
      testing documentation describes the durable contracts.

## Notes

### Pointers (as of 2026-08-23)

- `src/tui/receiver/queue.rs` currently owns the live 64-entry `VecDeque`,
  staged socket admission, and destructive head commit. Use it to preserve FIFO
  and admission invariants while moving authority into durable state.
- `src/tui/receiver/runtime.rs` owns receiver-local queue, lease, session,
  retry, and sync facts. Separate durable domain facts from live TUI
  observations instead of serializing this runtime wholesale.
- `src/server/receiver/job.rs` defines the immutable accepted job frame. Evolve
  the persisted representation without weakening workspace, actor, provider,
  attachment, or reply identity.
- `src/state/{mod,database,session_store}.rs` owns the existing workspace-scoped
  SQLite database and schema version. Add the durable receiver model behind a
  focused state submodule while preserving session-store behavior.
- `src/startup_migration/` and `docs/decisions.md` define the required automatic
  up/down migration and durable design record. The startup migration must
  reconcile existing workspace databases, while ordinary database open must
  initialize a newly attached workspace.

### Plan (2026-08-23)

1. Characterize receiver identity, lifecycle, and binding decisions with pure
   red tests, then add typed job and conversation domain values.
2. Characterize durable acceptance, deduplication, claims, transitions,
   transcript/session binding, and restart persistence with SQLite red tests,
   then add the smallest transactional store that satisfies them.
3. Add red tests for automatic receiver-schema upgrade, reconciliation, and
   downgrade, then wire the schema through workspace database open and the
   startup migration lifecycle.
4. Update the durable data-model, architecture, integration, feature,
   decision, and testing contracts; bump the crate version and run the full
   formatting, release-test, and lint gates.

### Log

- 2026-08-15 created as the umbrella proposal to replace receiver injection
  with queued per-message sessions.
- 2026-08-23 narrowed into the durable model foundation for PROJ-1 after the
  queue, conversation, session, transcript, and recovery contracts were
  confirmed.
- 2026-08-23 completed in `f4d6280` and released as Brain 0.72.0. SQLite schema
  v6 now owns typed receiver conversations, durable job lifecycles, expiring
  non-destructive claims, retries, transcript updates, and frontend-native
  session bindings. The full release suite and release Clippy passed.
