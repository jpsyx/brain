---
prefix: BR
cadence_weeks: 2
current_cycle:
counters:
  task: 10
  project: 0
  initiative: 0
  milestone: 0
---

# Workspace config

Machine-readable settings live in the frontmatter above. Increment a counter
in `counters` after allocating an id of that type.

## Priorities

`none` < `low` < `medium` < `high` < `urgent`.

## Labels

Maintain the project's label taxonomy here. Seed:

- `bug` — broken behavior.
- `feature` — new user-facing capability.
- `enhancement` — improvement to existing behavior.
- `tech-debt` — internal cleanup / refactor.
- `chore` — maintenance, tooling, ops.
- `sync` — brain sync pipeline.
- `server` — brain HTTP server / receiver.

## Estimates

Fibonacci points: `1, 2, 3, 5, 8, 13`.
