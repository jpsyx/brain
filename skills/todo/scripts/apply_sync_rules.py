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
"""
import argparse
import csv
import json
import os
import re
import sys
from datetime import date
from pathlib import Path

BRAIN = Path(os.environ.get("BRAIN_ROOT", Path.home() / "brain")).expanduser()
TASKS = BRAIN / "tasks" / "tasks.csv"
HABITS = BRAIN / "tasks" / "habits.csv"
PROJECTS_DIR = BRAIN / "projects"

CHECKBOX_RE = re.compile(r"^\s*-\s*\[[ x]\]", re.MULTILINE)


def read_csv(path: Path):
    if not path.exists():
        return [], []
    with open(path, newline="") as f:
        reader = csv.DictReader(f)
        return reader.fieldnames or [], list(reader)


def write_csv(path: Path, columns, rows):
    with open(path, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=columns, quoting=csv.QUOTE_MINIMAL)
        w.writeheader()
        for r in rows:
            w.writerow({c: r.get(c, "") for c in columns})


def touch_row(row: dict, today: str) -> None:
    row["last_touched"] = today


def load_json(path: Path):
    with open(path) as f:
        return json.load(f)


def save_json(path: Path, data):
    with open(path, "w") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")


def project_meta_paths():
    if not PROJECTS_DIR.is_dir():
        return []
    return sorted(PROJECTS_DIR.glob("*/.METADATA.json"))


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--fix", action="store_true",
                   help="write corrections (default: dry-run, report only)")
    args = p.parse_args()

    today = date.today().isoformat()
    issues = []
    fixes_applied = []

    # 1-8 task-level rules ------------------------------------------------
    for path in (TASKS, HABITS):
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
            if path == TASKS and "habit" in (r.get("task_type") or ""):
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
    for path in (TASKS, HABITS):
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
        meta_path = PROJECTS_DIR / slug / ".METADATA.json"
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
