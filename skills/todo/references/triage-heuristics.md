# Triage heuristics

How `/todo triage` groups past-due tasks for bulk decisions. The user
can have 50+ past-due tasks; going 1-by-1 is unworkable. The skill
should always group first and offer bulk operations before walking
items individually.

## When to invoke

- User says: "triage my tasks", "run triage", "morning triage",
  "let's clean up past-due", "what's slipping?"
- Top of `/todo` agenda when there are 10+ past-due tasks.

## Step 1 — Pick the most useful grouping

Try these in order; commit to the **first** one whose largest bucket
has at least 3 items (otherwise you're just walking 1-by-1 anyway):

1. **By `task_type`** — "8 code tasks past due", "12 personal
   tasks past due". This is usually the most natural grouping.
2. **By overdue bucket** — `<7d`, `7-30d`, `>30d`. Surfaces the
   "ancient zombies" (>30d) for a separate question.
3. **By `priority`** — useful when type-grouping is balanced but
   priority is skewed.
4. **By `project`** — when many past-due tasks share a project.

If multiple groupings tie, prefer the one that gives the smallest
number of groups (less cognitive load for the user).

## Step 2 — Present the groups

One line per group:

```
Past-due triage (47 tasks):

  personal  (18) — defer all 7d / drop all / 1-by-1 / skip
  code      (12) — defer all 7d / drop all / 1-by-1 / skip
  ceo       (8)  — defer all 7d / drop all / 1-by-1 / skip
  finance   (4)  — defer all 7d / drop all / 1-by-1 / skip
  languages (3)  — defer all 7d / drop all / 1-by-1 / skip
  > 30d old (12) — defer all 7d / drop all / 1-by-1 / skip
```

The "> 30d old" cross-cut is shown even when the primary grouping
is by task_type — it's the highest-signal group.

## Step 3 — Bulk operations per group

- **Defer all N days** — accept any positive integer; quick-picks
  are `1`, `7`, `14`. Increments `defer_count` for each. If any task
  in the group hits `defer_count >= 3` *after* the bulk defer, flag
  it in the summary.
- **Drop all** — confirms with the count, then removes them from the
  CSV. Never silently destructive.
- **1-by-1** — walks the group with per-task prompts (see Step 4).
- **Skip group** — moves to next.

## Step 4 — 1-by-1 walk

For each task in the chosen group:

```
[3/12] "Add npm run type-check to release..."
  type: code   priority: p2   due: 2026-05-08 (30d ago)
  defer_count: 0
  notes: (none)

  done / defer +7d / defer +14d / defer to date / drop / skip / mit / change-priority
```

**High-defer warning**: if `defer_count >= 3` *before* this action,
prepend a line in red/bold:

```
  ⚠ deferred 4 times already — drop it, or commit to a firm date?
```

Recognize when the user wants to convert to a project (see
[task-project-link.md](task-project-link.md) conversion triggers).
If `estimated_duration > 90` or the task has scope verbs, suggest
`/todo turn-into-project` instead of deferring again.

## Step 5 — Summary

After triage:

```
Triage complete:
  - 12 tasks deferred (avg push: 7d)
  - 3 tasks dropped
  - 1 task converted to project: apply-to-ict4d-conference
  - 0 tasks marked MIT
  - 4 high-defer warnings issued

  Remaining past-due: 0
```

## Never auto-mutate

Triage never deletes, defers, or re-prioritizes anything without an
explicit user choice. Bulk options count as explicit consent for the
**entire group**.

## Hard-deadline tasks

Tasks with `hard_deadline=true` are flagged in the per-task display
and excluded from "defer all" bulk operations. The user must defer
them individually with confirmation.
