---
id: BR-14
title: Run receiver jobs in isolated resumable tabs
status: backlog
priority: high
assignee: jpsyx
labels: [feature, server]
estimate: 8
project: PROJ-1
milestone: MS-2
cycle:
parent:
github:
blocked_by: []
created: 2026-08-23
updated: 2026-08-23
---

# BR-14: Run receiver jobs in isolated resumable tabs

## Description

Consume the oldest ready durable job in a dedicated ephemeral brain-panel tab
with its own agent process. Receiver work never types into the main interactive
panel or another running receiver process, never waits for the main panel to
become idle, and never changes the user's active view or focus.

For a known conversation, launch a new process that resumes its stored native
frontend session and supplies the new job as the initial prompt. If that session
is unavailable, corrupt, belongs to another frontend, or cannot be resumed,
start a fresh native session whose prompt references the Brain-owned transcript.
Record the replacement binding and close the process/tab after the run reaches a
terminal lifecycle outcome.

## Acceptance criteria

- [ ] The consumer atomically claims, rather than removes, the oldest ready job.
- [ ] Every job runs in a dedicated remote-run tab and a newly launched agent
      process through `AgentController`.
- [ ] A resumable matching conversation uses `Resume(session_id)` plus the job
      as the initial prompt for Claude, Codex, and OpenCode.
- [ ] Missing or incompatible native history starts fresh with the bounded
      Brain transcript and updates the conversation's native binding.
- [ ] No receiver path injects text into an existing process or waits for the
      main interactive session.
- [ ] Remote tabs run in the background without stealing view, tab, or keyboard
      focus and close themselves after a terminal outcome.
- [ ] A message arriving while a remote run is active remains durable and is
      considered only after the current run closes.
- [ ] One workspace runs at most one receiver job concurrently in this release.
- [ ] Red/green tests cover resume, transcript fallback, frontend changes,
      background focus, mid-run arrival, launch failure, and FIFO draining.
- [ ] Brain-panel, skill-session, receiver, glossary, feature, integration,
      architecture, keybinding, decision, and testing docs stay consistent.

## Notes

### Pointers (as of 2026-08-23)

- `src/tui/app_skill_session/` is the closest working pattern for a fresh
  single-prompt process in an auto-closing tab. Extract or share the generic
  lifecycle without representing receiver runs as configured skill sessions.
- `src/tui/state/brain.rs` owns main and ephemeral tab controllers. Add a
  distinct receiver-run representation while preserving monotonic tab identity
  and failure-safe controller shutdown.
- `src/agent/frontend.rs`, `src/agent/session.rs`, and the Claude, Codex, and
  OpenCode adapters already accept a `SessionPlan` and initial prompt. Keep the
  launch decision behind `AgentController`.
- `src/tui/app_brain/receiver/dispatch.rs` is the legacy fresh-launch and
  warm-panel-injection coordinator. Replace its ownership rather than layering
  the new tab path alongside it indefinitely.

### Log

- 2026-08-23 created from PROJ-1 planning.
- 2026-08-23 unblocked after BR-12 and BR-13 shipped the durable model and
  ingress acceptance boundary. This is the next PROJ-1 implementation task.
