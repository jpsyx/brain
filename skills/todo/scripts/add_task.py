#!/usr/bin/env python3
"""Append a new task row.

Most calls come from /todo via an LLM that has already parsed the user's
intent into structured fields. Required: --name, --type, --priority.
All other fields are optional and default to empty (or 0 for defer_count).

Usage:
    add_task.py --name "Email Aseel about Q3" --type ceo --priority p1 --due 2026-06-10
    add_task.py --name "Workout" --type personal --priority p1 --habit --interval 1 --unit days
    add_task.py --name "Draft whitepaper" --type code --priority p1 --due 2026-07-01 \\
        --duration 30 --chunks 5
        # → creates "Draft whitepaper (1/5)" … "(5/5)", each 30 min, same due
        #   date, sequential blocked_by. See SKILL.md "Chunked tasks".
"""
import argparse
import sys
from _csvlib import (
    HABITS_CSV, TASKS_CSV, new_habit_id, new_task_id, read_csv, today_iso, write_csv,
)

TASK_COLS = [
    "task_id", "task_name", "task_type", "status", "waiting_since", "priority",
    "due_date", "hard_deadline", "start_date", "assignee", "see_also",
    "notes", "project", "energy_level", "context",
    "estimated_duration", "blocked_by", "defer_count",
    "created_date", "completed_date", "last_touched", "linear_issue",
]
HABIT_COLS = [
    "task_id", "task_name", "status", "priority", "due_date",
    "hard_deadline", "assignee", "see_also", "notes", "project",
    "energy_level", "context", "estimated_duration",
    "recur_interval", "recur_unit",
    "created_date", "completed_date", "last_touched",
]


def _strip_mit(task_type: str) -> str:
    return "|".join(t for t in (task_type or "").split("|") if t and t != "mit")


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--name", required=True)
    p.add_argument("--type", help="pipe-sep enum-set; e.g. 'code|mit'")
    p.add_argument("--priority", required=True, choices=["p0", "p1", "p2", "p3", "p4"])
    p.add_argument("--due", default="")
    p.add_argument("--start", default="")
    p.add_argument("--hard-deadline", action="store_true")
    p.add_argument("--see-also", default="")
    p.add_argument("--notes", default="")
    p.add_argument("--project", default="")
    p.add_argument("--energy", choices=["high", "medium", "low"], default="")
    p.add_argument("--context", choices=["home", "office", "computer", "calls", "errand"], default="")
    p.add_argument("--duration", default="", help="estimated duration in minutes (per-chunk when --chunks is set)")
    p.add_argument("--blocked-by", default="")
    p.add_argument("--assignee", default="me")
    p.add_argument("--linear-issue", default="",
                   help="Linear issue identifier (e.g. AVA-123) for a code task already filed "
                        "in Linear. Usually set later via set_linear_issue.py once the issue "
                        "exists. With --chunks, applied to chunk 1 only (chunks are one issue).")
    # habit-only:
    p.add_argument("--habit", action="store_true", help="route to habits.csv")
    p.add_argument("--interval", type=int, help="recur_interval (required for habits)")
    p.add_argument("--unit", choices=["days", "weeks", "months"], help="recur_unit (required for habits)")
    # chunked-task-only:
    p.add_argument("--chunks", type=int, default=0,
                   help="split into N sequential chunks (each --duration minutes). "
                        "Names become '<name> (i/N)'; chunk i+1 is blocked_by chunk i.")
    args = p.parse_args()

    common = {
        # task_id is assigned below per-kind (T### for tasks, H### for habits)
        "task_name": args.name,
        "status": "not_started",
        "priority": args.priority,
        "due_date": args.due,
        "hard_deadline": "true" if args.hard_deadline else "false",
        "assignee": args.assignee,
        "see_also": args.see_also,
        "notes": args.notes,
        "project": args.project,
        "energy_level": args.energy,
        "context": args.context,
        "estimated_duration": args.duration,
        "created_date": today_iso(),
        "completed_date": "",
        "last_touched": today_iso(),
    }

    if args.habit:
        if args.chunks:
            print("--chunks is not supported with --habit (habits recur, they don't chunk)", file=sys.stderr)
            return 2
        if not (args.interval and args.unit):
            print("--habit requires --interval and --unit", file=sys.stderr)
            return 2
        path = HABITS_CSV
        cols = HABIT_COLS
        row = {**common,
               "task_id": new_habit_id(),
               "recur_interval": str(args.interval),
               "recur_unit": args.unit}
        new_rows = [row]
    else:
        if not args.type:
            print("--type is required for non-habit tasks", file=sys.stderr)
            return 2
        path = TASKS_CSV
        cols = TASK_COLS

        if args.chunks:
            if args.chunks < 2:
                print("--chunks must be >= 2 (a single 'chunk' is just a normal task)", file=sys.stderr)
                return 2
            if not str(args.duration).strip():
                print("--chunks requires --duration (per-chunk minutes)", file=sys.stderr)
                return 2
            # Inheritance per SKILL.md "Chunked tasks":
            #   - hard_deadline: all chunks (set via `common`)
            #   - mit:           only chunk 1; brain tasks complete migrates it forward
            #   - blocked_by:    user-supplied on chunk 1; chunks 2..N point at the previous chunk
            base_type = args.type or ""
            type_without_mit = _strip_mit(base_type)
            new_rows = []
            prev_id = ""
            for i in range(1, args.chunks + 1):
                tid = new_task_id()
                if i == 1:
                    task_type = base_type
                    blocked = args.blocked_by
                    linear = args.linear_issue
                else:
                    task_type = type_without_mit
                    blocked = prev_id
                    linear = ""
                new_rows.append({**common,
                                 "task_id": tid,
                                 "task_name": f"{args.name} ({i}/{args.chunks})",
                                 "task_type": task_type,
                                 "start_date": args.start,
                                 "blocked_by": blocked,
                                 "defer_count": "0",
                                 "linear_issue": linear})
                prev_id = tid
        else:
            new_rows = [{**common,
                         "task_id": new_task_id(),
                         "task_type": args.type,
                         "start_date": args.start,
                         "blocked_by": args.blocked_by,
                         "defer_count": "0",
                         "linear_issue": args.linear_issue}]

    existing_cols, rows = read_csv(path)
    if not existing_cols:
        existing_cols = cols
    rows.extend(new_rows)
    write_csv(path, existing_cols, rows)

    def _is_pr_review(r):
        """PR-review tasks (e.g. 'Review PR: …' / 'Review PR #…') are NEVER
        filed in Linear — one-way Linear/GitHub → todo only. An empty
        linear_issue on them is correct, not drift. See
        todo/references/linear-link.md 'PR-review tasks'."""
        name = (r.get("task_name") or "").lstrip()
        return name.lower().startswith("review pr")

    def _linear_hint(r):
        """Surface the code-task <-> Linear obligation. The link is LLM-mediated
        (scripts can't reach the Linear MCP) — see todo/references/linear-link.md."""
        is_code = "code" in (r.get("task_type") or "").split("|")
        if r.get("linear_issue"):
            print(f"  🔗 linear issue {r['linear_issue']} linked")
        elif is_code and not _is_pr_review(r):
            print(f"  ⚠ code task with no Linear issue — file it in Linear (via /linear-pm "
                  f"for placement), then `set_linear_issue.py {r['task_id']} <AVA-###>`")

    if len(new_rows) == 1:
        r = new_rows[0]
        print(f"added: {r['task_id']}  {r['task_name']}  → {path.name}")
        if r.get("project"):
            print(f"  ⓘ project link '{r['project']}' — run /todo sync --fix to mirror into .METADATA.json")
        if path == TASKS_CSV:
            _linear_hint(r)
    else:
        print(f"added {len(new_rows)} chunk(s) → {path.name}:")
        for r in new_rows:
            blocker = r.get("blocked_by") or "(none)"
            print(f"  {r['task_id']}  {r['task_name']}  [blocked_by: {blocker}]")
        if new_rows[0].get("project"):
            print(f"  ⓘ project link '{new_rows[0]['project']}' — run /todo sync --fix to mirror into .METADATA.json")
        if path == TASKS_CSV:
            _linear_hint(new_rows[0])
    return 0


if __name__ == "__main__":
    sys.exit(main())
