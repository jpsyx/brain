---
id: BR-15
title: Prove receiver job acceptance and progress
status: backlog
priority: high
assignee: jpsyx
labels: [enhancement, server]
estimate: 8
project: PROJ-1
milestone: MS-3
cycle:
parent:
github:
blocked_by: [BR-14]
created: 2026-08-23
updated: 2026-08-23
---

# BR-15: Prove receiver job acceptance and progress

## Description

Give every remote run an opaque job token and observe whether that exact prompt
entered the frontend's conversation. Process spawn, PTY availability, typed
bytes, or screen activity alone must never count as prompt acceptance.

Expose one frontend-neutral observation contract through `AgentController`.
Each adapter may use its strongest reliable prompt-submit event, transcript,
rollout, or session evidence, but it must correlate evidence to the exact job
token and advance through a bounded cursor rather than repeatedly scanning an
entire growing history. Completion remains a separate token-matched fact.

## Acceptance criteria

- [ ] Every launch carries one unique job token in trusted launch metadata and
      in the exact prompt content that frontend evidence can correlate.
- [ ] `launched`, `accepted`, `progressing`, and `completed` are distinct
      observations with explicit persisted transitions.
- [ ] An exact prompt-submit or transcript event is required before a job is
      marked accepted.
- [ ] Claude, Codex, and OpenCode provide equivalent semantic observations
      behind `AgentController`; frontend storage grammar does not leak into the
      receiver coordinator.
- [ ] Observation is incremental and bounded for growing histories, and handles
      truncation, missing files, malformed entries, delayed writes, and session
      rotation conservatively.
- [ ] A valid token-matched completion may advance directly even when an
      intermediate observation was missed.
- [ ] Diagnostic records name the job and lifecycle boundary without logging
      prompt contents, provider credentials, or private response text.
- [ ] Red/green tests cover unobserved launch, exact acceptance, unrelated old
      messages, progress, malformed/ambiguous evidence, rotation, and missed
      intermediate observations without fixed sleeps.
- [ ] Agent, receiver, integration, architecture, decision, data-model, and
      testing docs describe the evidence contract.

## Notes

### Pointers (as of 2026-08-23)

- `src/agent/` owns frontend-specific transcript discovery, lifecycle bridges,
  and the semantic controller facade. Extend that boundary instead of matching
  frontend kinds inside the TUI.
- `src/agent/claude.rs`, `src/agent/codex/sessions.rs`, and
  `src/agent/opencode/session.rs` contain current resumability evidence and are
  the starting point for exact-token observation.
- `scripts/agent_session_start_hook.py`,
  `scripts/agent_session_stop_hook.py`, and
  `scripts/opencode_brain_plugin.js` bridge frontend lifecycle facts. Preserve
  the common authenticated workspace/actor/job lineage.
- `src/tui/app_brain/receiver/diagnostics.rs` currently samples the PTY because
  injection provides no stronger proof. Replace screen inference with exact
  lifecycle evidence while retaining useful redacted diagnostics.

### Log

- 2026-08-23 created from PROJ-1 planning and absorbs BR-10's exact-submission
  observation requirement.
