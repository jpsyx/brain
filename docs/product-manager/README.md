# Product board

This directory is the project's planning system, managed by the
`product-manager` skill. It recreates Linear's object model
(initiatives -> projects -> milestones -> cycles -> tasks) in plain
markdown, committed to the repo. There is no Linear connection.

## Layout

- `config.md` — cadence, current cycle, labels, priorities, id counters.
- `team.md` — assignable people.
- `initiatives/` — strategic goals spanning projects.
- `projects/` — bodies of work; milestones live inside each project file.
- `cycles/` — 2-week iterations (sprints), with plan + retro.
- `tasks/` — atomic work items, one file each.
- `archive/` — done/cancelled tasks.

## How it works

- Each entity is one markdown file with YAML frontmatter (the metadata) and
  a body (description, acceptance criteria, notes).
- The entity files are the single source of truth. Board, roadmap, and
  velocity views are computed on demand, not stored.
- New tasks default-assign to whoever added them and start in `backlog`
  until triaged.

Talk to the `product-manager` skill in natural language to add, edit,
triage, plan cycles, generate status updates, or ask "what should we work
on?".
