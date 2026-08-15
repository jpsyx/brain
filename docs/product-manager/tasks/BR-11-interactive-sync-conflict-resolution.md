---
id: BR-11
title: Resolve sync conflicts interactively with an LLM handoff
status: backlog
priority: none
assignee: jpsyx
labels: [feature, enhancement, sync, server]
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

# BR-11: Resolve sync conflicts interactively with an LLM handoff

## Description

Make sync conflict handling an explicit, user-guided part of every sync. When
`brain sync` finishes and conflict copies exist, it must immediately enter an
interactive resolution flow and ask what to do for each conflict, so a
successful sync never silently leaves unresolved copies behind. The final
choice for any conflict is LLM resolution.

Expose the same flow through the existing `brain sync resolve` command (the
preferred spelling over a nested `conflicts resolve` alias). With no explicit
originals, it should discover current conflicts and prompt at each step. User
choices that can be completed locally should be applied during the flow. LLM
choices should be collected into a queue, and only after all interactive
questions are complete should Brain hand the queued files to an agent session.

If no TUI is running, open it, start a new session, and inject one resolution
prompt containing the queued files and the required resolution context. If a
TUI is already running, send that prompt through the server to the running
workspace session. The handoff must use the existing frontend-neutral agent
controller/server delivery path and work with Claude, Codex, and OpenCode.

## Acceptance criteria

- [ ] A normal `brain sync` detects remaining conflict copies after the sync and immediately starts the interactive resolver; it does not report the run as complete while conflicts remain unresolved.
- [ ] `brain sync resolve` is a documented command that discovers conflicts when no originals are supplied and asks the user for a decision separately for every conflict.
- [ ] The interactive choices include the existing local resolution actions and a clearly labeled `LLM resolution` option; every non-LLM choice is applied and verified before advancing.
- [ ] The resolver handles an empty conflict set cleanly and rechecks for leftovers after local decisions, with no silent unresolved-conflict path.
- [ ] LLM selections are accumulated in an in-memory queue during the interactive pass; no agent session starts before all conflict questions have been answered.
- [ ] For a non-empty LLM queue, Brain builds a single prompt that names every conflict and its canonical file, opens the TUI with a fresh session when needed, and auto-injects the prompt.
- [ ] When a matching TUI/server lease is already running, the same prompt is delivered through the server to that workspace instead of opening a second TUI; delivery and failure are reported clearly.
- [ ] The LLM handoff is routed through `AgentController` and preserves equivalent lifecycle and completion behavior for Claude, Codex, and OpenCode.
- [ ] CLI help, interactive output, sync logs, and the relevant sync/server/TUI documentation describe the new flow and its no-conflict invariant.
- [ ] Pure decision and prompt-building logic has unit tests, including per-conflict choice sequencing, empty queues, multiple LLM files, running-TUI delivery, and fresh-session launch.

## Notes

### Pointers (as of 2026-08-15)

High-level guide to where and how to complete this, not a detailed plan
(references drift before the task is picked up). 1-2 sentences per item.

- `src/sync/command/mod.rs` and `src/sync/command/resolve.rs` — the sync orchestration and existing `brain sync resolve` interactive/delete flow. Extend the post-sync handoff and keep filesystem/rclone effects behind thin command code.
- `src/sync/conflicts/mod.rs` and `src/sync/command/reporting.rs` — conflict-copy discovery, grouping, display, and JSON reporting. Reuse these canonical conflict groups for deterministic per-conflict prompts and the final leftover check.
- `src/server/` and `src/server/routes/` — machine-wide server lifecycle, workspace routing, and HTTP route seams. Add the narrowest authenticated prompt-delivery route or control message needed for an already-running TUI, following the existing route-module pattern.
- `src/tui/app_brain/`, `src/tui/app_state/`, and `src/agent/` — fresh-session startup, prompt injection, and the frontend-neutral `AgentController`. Reuse these paths so queued LLM work behaves identically across frontends.
- `docs/features.md`, `docs/integrations.md`, `docs/architecture.md`, `docs/data-model.md`, and `docs/decisions.md` — update the user-visible no-conflict invariant, command surface, server/TUI handoff, queue semantics, and rationale together with the implementation.
- `docs/testing.md` and existing sync/server/TUI test modules — follow the repository's red/green pure-function strategy, adding focused tests before production changes.

### Log

- 2026-08-15 created.
