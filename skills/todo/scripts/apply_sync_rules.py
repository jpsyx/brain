#!/usr/bin/env python3
"""Apply all task automation/sync rules to tasks.csv + habits.csv.

Rules enforced (see ../references/sync-rules.md for the canonical doc):

1. completed_date is set when status=done and date is empty.
2. past_due is computed on read (not stored).
3. is_mit / is_done are derived (not stored).
4. defer_count is integer, defaults to 0.
5. Tasks with task_type containing 'habit' are flagged (they belong in
   habits.csv, not tasks.csv).
6. Project bidirectional link (tasks.project ↔ project .METADATA.json
   `tasks[]`). With --fix, missing reverse links are written; orphans
   are reported. See ../references/task-project-link.md.
7. No-sub-tasks check: notes containing '- [ ]' Markdown-style checkboxes
   are flagged with a hint to /todo turn-into-project.
8. last_touched: if column is missing it is added;
   empty values are backfilled from created_date (fallback: today).

Usage:
    apply_sync_rules.py             # dry-run; report only
    apply_sync_rules.py --fix       # write corrections
    apply_sync_rules.py --complete-managed-triage daily
"""
import argparse
import json
import re
import sys
from datetime import date
from pathlib import Path

from _csvlib import (
    brain_root,
    habits_csv,
    new_habit_id,
    new_uuid,
    read_csv,
    read_json,
    tasks_csv,
    touch_row,
    write_csv,
    write_json,
)
from next_habit_occurrence import next_due

CHECKBOX_RE = re.compile(r"^\s*-\s*\[[ x]\]", re.MULTILINE)
MANAGED_TRIAGE_KEYS = {
    "daily": "brain.triage.daily",
    "weekly": "brain.triage.weekly",
}


def load_json(path: Path):
    return read_json(path)


def save_json(path: Path, data):
    write_json(path, data)


def project_meta_paths():
    projects_dir = brain_root() / "projects"
    if not projects_dir.is_dir():
        return []
    return sorted(projects_dir.glob("*/.METADATA.json"))


def triage_habits_enabled() -> bool:
    path = brain_root() / ".config" / "config.json"
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return True
    return value.get("enable_triage_habits", True) is not False


def complete_managed_triage(kind: str) -> int:
    if not triage_habits_enabled():
        print("managed triage habits are disabled; workflow completion recorded without habit mutation")
        return 0

    path = habits_csv()
    columns, rows = read_csv(path)
    key = MANAGED_TRIAGE_KEYS[kind]
    pending = [
        row
        for row in rows
        if (row.get("system_key") or "").strip() == key
        and (row.get("status") or "").strip() != "done"
    ]
    if len(pending) != 1:
        raise SystemExit(
            f"expected exactly one pending {kind} managed triage habit; "
            "run `brain reindex --tasks` to reconcile definitions"
        )

    today = date.today().isoformat()
    source = pending[0]
    source["status"] = "done"
    source["completed_date"] = today
    touch_row(source, today)
    occurrence = dict(source)
    occurrence["task_uuid"] = new_uuid()
    occurrence["task_id"] = new_habit_id()
    occurrence["status"] = "not_started"
    occurrence["due_date"] = next_due(
        source["due_date"],
        int(source["recur_interval"]),
        source["recur_unit"],
    )
    occurrence["created_date"] = today
    occurrence["completed_date"] = ""
    occurrence["last_touched"] = today
    rows.append(occurrence)
    write_csv(path, columns, rows)
    print(f"completed managed {kind} triage habit {source.get('task_id')}")
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--fix", action="store_true",
                   help="write corrections (default: dry-run, report only)")
    p.add_argument(
        "--complete-managed-triage",
        choices=sorted(MANAGED_TRIAGE_KEYS),
        help="complete Brain's protected daily or weekly triage occurrence",
    )
    args = p.parse_args()

    if args.complete_managed_triage:
        return complete_managed_triage(args.complete_managed_triage)

    today = date.today().isoformat()
    issues = []
    fixes_applied = []

    # 1-8 task-level rules ------------------------------------------------
    tasks = tasks_csv()
    habits = habits_csv()
    for path in (tasks, habits):
        if not path.exists():
            continue
        cols, rows = read_csv(path)
        changed = False

        # rule 8: last_touched column
        if "last_touched" not in cols:
            if args.fix:
                cols = cols + ["last_touched"]
                fixes_applied.append(f"{path.name}: added last_touched column")
                changed = True
            else:
                issues.append(
                    f"{path.name}: missing last_touched column "
                    f"(--fix will add it and backfill {len(rows)} row(s) from created_date)"
                )

        for r in rows:
            # rule 1: completed_date when done
            if r.get("status") == "done" and not (r.get("completed_date") or "").strip():
                if args.fix:
                    r["completed_date"] = today
                    touch_row(r, today)
                    fixes_applied.append(f"{path.name}: set completed_date on '{r.get('task_id')} {r.get('task_name')}'")
                    changed = True
                else:
                    issues.append(f"{path.name}: '{r.get('task_id')} {r.get('task_name')}' is done but completed_date is empty")
            # rule 4: defer_count defaults to 0
            if "defer_count" in cols:
                v = (r.get("defer_count") or "").strip()
                if not v:
                    if args.fix:
                        r["defer_count"] = "0"
                        touch_row(r, today)
                        changed = True
                    else:
                        issues.append(f"{path.name}: '{r.get('task_id')} {r.get('task_name')}' has empty defer_count")
            # rule 5: misplaced habit
            if path == tasks and "habit" in (r.get("task_type") or ""):
                issues.append(f"tasks.csv: '{r.get('task_id')} {r.get('task_name')}' has task_type=habit — should move to habits.csv")
            # rule 7: sub-tasks → project
            if CHECKBOX_RE.search(r.get("notes") or ""):
                issues.append(
                    f"{path.name}: '{r.get('task_id')} {r.get('task_name')}' has sub-task checkboxes in notes "
                    f"— consider /todo turn-into-project"
                )

        # rule 8 (cont.): backfill empty last_touched
        if "last_touched" in cols:
            missing_lt = [r for r in rows if (r.get("last_touched") or "").strip() == ""]
            if missing_lt:
                if args.fix:
                    for r in missing_lt:
                        r["last_touched"] = (r.get("created_date") or "").strip() or today
                    fixes_applied.append(f"{path.name}: backfilled last_touched on {len(missing_lt)} row(s) (from created_date)")
                    changed = True
                else:
                    issues.append(f"{path.name}: {len(missing_lt)} row(s) have empty last_touched (run with --fix to backfill from created_date)")

        if args.fix and changed:
            write_csv(path, cols, rows)

    # 6. project bidirectional link ---------------------------------------
    forward = {}  # project_slug -> set of task_ids referencing it
    task_meta = {}  # task_id -> (path, row)
    for path in (tasks, habits):
        if not path.exists():
            continue
        _, rows = read_csv(path)
        for r in rows:
            tid = r.get("task_id")
            if not tid:
                continue
            task_meta[tid] = (path, r)
            slug = (r.get("project") or "").strip()
            if slug:
                forward.setdefault(slug, set()).add(tid)

    # forward: every task.project must point to an existing project dir
    for slug, tids in forward.items():
        meta_path = brain_root() / "projects" / slug / ".METADATA.json"
        if not meta_path.exists():
            issues.append(
                f"orphan task→project: project '{slug}' (referenced by {len(tids)} task(s)) does not exist"
            )

    # reverse: every project .METADATA.json's tasks[] must hit a real task_id
    for meta_path in project_meta_paths():
        meta = load_json(meta_path)
        slug = meta.get("name") or meta_path.parent.name
        listed = list(meta.get("tasks") or [])
        listed_set = set(listed)
        expected_set = forward.get(slug, set())

        missing_in_meta = expected_set - listed_set       # task points to project, project doesn't know
        missing_in_csv = listed_set - expected_set        # project lists task, task doesn't point back

        if missing_in_meta:
            if args.fix:
                meta["tasks"] = sorted(listed_set | missing_in_meta)
                save_json(meta_path, meta)
                fixes_applied.append(
                    f"projects/{slug}: added {len(missing_in_meta)} task_id(s) to .METADATA.json"
                )
            else:
                issues.append(
                    f"link mismatch: {len(missing_in_meta)} task(s) point to project '{slug}' "
                    f"but .METADATA.json doesn't list them"
                )
        if missing_in_csv:
            issues.append(
                f"orphan project→task: project '{slug}' lists task_id(s) {sorted(missing_in_csv)} "
                f"that don't exist in any CSV"
            )

    # report --------------------------------------------------------------
    if args.fix and fixes_applied:
        print(f"Applied {len(fixes_applied)} fix(es):")
        for line in fixes_applied:
            print(f"  + {line}")
    if issues:
        print(f"\n{len(issues)} issue(s):")
        for line in issues:
            print(f"  ! {line}")
        return 0 if args.fix else 1
    if not args.fix and not fixes_applied:
        print("sync rules: all clean.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
