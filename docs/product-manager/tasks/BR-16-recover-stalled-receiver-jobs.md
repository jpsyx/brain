---
id: BR-16
title: Recover stalled receiver jobs without blocking the queue
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
blocked_by: [BR-15]
created: 2026-08-23
updated: 2026-08-23
---

# BR-16: Recover stalled receiver jobs without blocking the queue

## Description

Add a recurring durable reconciler that turns lifecycle observations into
bounded recovery decisions. A job never observed as accepted can be terminated
and safely requeued. A job proven accepted but no longer progressing receives
one automatic recovery attempt by launching a new process that resumes the same
logical session and names the same job. A second failure records a terminal
failure, notifies the sender, and releases the queue for later work.

Use persisted claim and progress leases so the same rules reconcile jobs after
a TUI, shared server, agent process, or machine restart. Event delivery may make
the common path immediate, but polling must recover missed events and stale
ownership.

## Acceptance criteria

- [ ] Launch, acceptance, progress, and recovery leases are explicit,
      persisted, and evaluated through pure decisions with injected clocks.
- [ ] A launch that never proves acceptance is terminated and requeued without
      consuming its one accepted-job recovery attempt.
- [ ] A job proven accepted but stalled receives exactly one automatic recovery
      attempt in the same logical/native session when resumable.
- [ ] Recovery instructions name the same job and ask the resumed conversation
      to reconcile prior work rather than blindly repeat side effects.
- [ ] A second accepted-job failure records a terminal failure, sends the
      sender a clear unavailable response, and allows the next job to run.
- [ ] Process exit, missing evidence, corrupt native history, and TUI or machine
      restart use deterministic recovery rules rather than leaving an active
      claim indefinitely.
- [ ] Startup reconciles stale claims and answer/delivery states before claiming
      new work.
- [ ] Progress-renewed leases have an absolute upper bound so continuously
      ambiguous state cannot block the queue forever.
- [ ] Red/green tests cover every timeout boundary, missed event, restart point,
      safe requeue, one recovery, terminal failure, sender notice, and queue
      advancement without wall-clock sleeps.
- [ ] BR-10's old screen-based inactivity watchdog is removed or narrowed to
      non-authoritative diagnostics, with applicable docs updated.

## Notes

### Pointers (as of 2026-08-23)

- `src/tui/receiver/policy.rs` and `src/tui/receiver/decision.rs` contain the
  current pure timeout, retry, and ordered-stage decisions. Preserve injected
  time and explicit effects while replacing process-local facts with durable
  job observations.
- `src/tui/receiver/runtime/` coordinates active turn, retry, freshness, and
  queue state. The new reconciler should consume persisted snapshots and return
  semantic effects rather than own controllers or provider IO.
- `src/tui/app_brain/receiver/completion.rs` contains the five-minute
  abandonment behavior that currently prevents a wedged injected turn from
  blocking forever. Replace it only after every durable recovery transition is
  covered.
- `docs/product-manager/archive/BR-10-*.md` records the superseded recovery
  proposal and the failure cases this task absorbs.

### Log

- 2026-08-23 created from PROJ-1 planning and absorbs BR-10's bounded recovery
  requirements.
