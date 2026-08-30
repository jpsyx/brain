---
id: PROJ-1
name: Rearchitect receiver processing
status: completed
health: on-track
lead: jpsyx
members: []
initiative:
target_date:
github:
created: 2026-08-23
updated: 2026-08-29
---

# PROJ-1: Rearchitect receiver processing

## Goal

Make every accepted SMS and email job durable, observable, recoverable, and
isolated from the interactive brain panel. A receiver job must never disappear
across a restart or block all later work because Brain cannot tell whether a
frontend actually began processing it.

## Scope

In scope:

- Persist jobs before acknowledging provider ingress, then claim them with
  expiring ownership rather than destructively popping them.
- Track one logical receiver conversation per workspace, portable user,
  channel, and conversation key. SMS uses one stable conversation for the
  workspace-user-channel tuple; email uses verified thread lineage and starts a
  new conversation when lineage is uncertain.
- Launch every job in a dedicated ephemeral tab and a new agent process. Resume
  the conversation's native frontend session with the job as its initial
  prompt when possible; use a continuously maintained Brain transcript to
  recover when native history is unavailable or the frontend changes.
- Prove that the exact job was accepted and is progressing through
  frontend-specific evidence behind `AgentController`.
- Give an accepted but stalled job one automatic recovery attempt in the same
  logical session. A second failure records the terminal failure, notifies the
  sender, and advances the queue.
- Persist answer readiness separately from provider delivery so a delivery
  retry never reruns agent work.
- Remove receiver injection, warm-panel reuse, and the coarse inactivity
  watchdog after the new path is complete.

Out of scope:

- Processing receiver jobs when no matching workspace TUI is live.
- Running more than one receiver job concurrently for a workspace.
- Treating frontend-owned session history as Brain's durable source of truth.
- Guessing that two emails share a conversation from subject text alone.
- Giving one SMS user multiple independent threads in the first release.

## Product contracts

- Provider acceptance means the job is durable.
- Process launch does not mean prompt acceptance.
- Prompt acceptance requires evidence for the exact job token.
- A queue head remains persisted until it reaches a terminal state.
- Native session history is the normal continuity path; the Brain transcript
  is the portable recovery path.
- Remote jobs never inject text into a running agent process, including a
  matching receiver process and the main interactive panel.
- Failure in processing, completion capture, or provider delivery cannot leave
  the queue indefinitely blocked.

## Milestones

- **MS-1: Durable jobs and conversations** (completed 2026-08-23): establish the
  persistent job, claim, conversation, transcript, and ingress contracts.
- **MS-2: Isolated resumable execution** (completed 2026-08-26): run queued jobs
  in dedicated ephemeral tabs without receiver injection.
- **MS-3: Verifiable lifecycle and recovery** (completed 2026-08-29): prove job
  acceptance and progress, recover one stalled attempt, and isolate response
  delivery retries.
- **MS-4: Injection-free cutover** (completed 2026-08-29): remove the legacy path,
  migrate existing state, and complete documentation and hardening.

## Task sequence

| Task | Milestone | Outcome |
| --- | --- | --- |
| BR-12 | MS-1 | Durable job and conversation model |
| BR-13 | MS-1 | Provider ingress commits jobs durably |
| BR-14 | MS-2 | Dedicated resumable receiver-run tabs |
| BR-15 | MS-3 | Exact acceptance and progress evidence |
| BR-16 | MS-3 | Bounded recovery and restart reconciliation |
| BR-17 | MS-3 | Durable answer and independent delivery retry |
| BR-18 | MS-4 | Legacy injection removal and verified cutover |

## Risks and decisions to verify during implementation

- Each frontend exposes different transcript and lifecycle evidence. The
  frontend registry must keep these differences behind one semantic controller
  contract and tests must characterize the actual installed command shapes.
- A job proven unaccepted can be retried safely. A job proven accepted must be
  recovered in the same logical session so Brain does not blindly duplicate
  agent side effects.
- The durable queue, transcript update, answer record, and delivery state need
  explicit crash boundaries. Recovery must reconcile persisted facts rather
  than infer them from a surviving tab.
- Email quotations can supplement context but cannot establish thread identity
  or replace Brain's transcript.

## Status updates

- **2026-08-23: on-track**: Product decisions confirmed. The former BR-12
  umbrella task has been split into seven ordered implementation tasks. BR-10
  is superseded; its useful acceptance, progress, and recovery requirements are
  carried by BR-15 and BR-16.
- **2026-08-23: on-track**: BR-12 shipped in Brain 0.72.0. Workspace-scoped
  receiver conversations, complete durable job states, expiring claims,
  transcript recovery, native session bindings, and automatic schema v6
  migrations are now established. BR-13 is the next ingress cutover step.
- **2026-08-23: on-track**: BR-13 shipped in Brain 0.73.2. Authenticated SMS
  and email ingress now commits exact workspace-scoped jobs before provider
  success, with restart-safe deduplication, atomic capacity, and crash recovery.
  MS-1 is complete and BR-14 is the next execution step.
- **2026-08-26: on-track**: BR-14 and BR-15 shipped through Brain 0.83.9.
  Receiver jobs now run in dedicated resumable tabs and advance only from
  bounded, exact token-matched frontend observations. BR-16 is next and will
  recover stalled launched, accepted, or processing work without blindly
  duplicating a prompt.
- **2026-08-27: on-track**: BR-16 shipped in Brain 0.84.22. Durable lifecycle
  deadlines now drive bounded pre-acceptance retry, one exact same-session
  accepted recovery, restart reconciliation, terminal unavailable notice, and
  FIFO advancement across cleanup failure. BR-17 is unblocked and will make
  answer persistence and provider delivery independently recoverable.
- **2026-08-29: on-track**: BR-17 shipped in Brain 0.85.28. Exact completion now
  atomically records the answer and transcript before independently claimed,
  immutable provider delivery. Retry, ambiguity, fallback, and restart
  reconciliation cannot rerun agent work, completing MS-3.
- **2026-08-29: on-track**: BR-18 shipped in Brain 0.86.13. Schema v13 removed
  the final legacy notice bridge, server routing moved to lease-only authority,
  permanent guards prohibit receiver injection, status and logs expose only
  redacted durable state, and restart recovery covers every lifecycle phase.
  MS-4 and PROJ-1 are complete.
