#!/usr/bin/env python3
"""Silently delete backlog tasks older than 6 months.

A task with `status = backlog` whose `backlogged_date` is MORE than 6
months ago (i.e. at least 6 months + 1 day old) is deleted outright. The
premise: a task parked and untouched for half a year has been forgotten
and is fine to forget forever.

DELIBERATELY SILENT. By design this script:
  - does NOT warn before deleting,
  - does NOT announce which tasks were deleted,
  - prints nothing to stdout unless `--report` is passed (debug only).
The /triage skill runs it plain so nothing leaks to the user.

PROJECT BOOKKEEPING (the one thing it is NOT silent about, internally):
if a deleted task belonged to a project — active OR archived — we leave a
breadcrumb in that project so a future un-archive knows tasks used to
exist:
  - `.METADATA.json` gains a `deleted_backlog_tasks` array entry
    {task_id, task_name, backlogged_date, deleted_date}, and the id is
    removed from the live `tasks` array;
  - a line is appended to the project's notes.md (created if absent) under
    a "Deleted backlog tasks" heading.

Usage:
    purge_old_backlog.py            # silent purge (normal triage use)
    purge_old_backlog.py --report   # purge + print a JSON summary (debug)
    purge_old_backlog.py --dry-run  # report what WOULD be deleted, change nothing
"""
import argparse
import json
import sys
from datetime import date

from _csvlib import (
    brain_root,
    parse_date,
    read_csv,
    read_json,
    tasks_csv,
    write_csv,
    write_json,
)


def minus_six_months(d: date) -> date:
    """Date six calendar months before d, clamping the day for short months."""
    month = d.month - 6
    year = d.year
    while month <= 0:
        month += 12
        year -= 1
    # clamp day (e.g. Aug 31 - 6mo would be Feb 31 -> Feb 28/29)
    day = d.day
    while True:
        try:
            return date(year, month, day)
        except ValueError:
            day -= 1


def find_project_dir(slug: str):
    """Locate a project directory by slug under projects/ or archive/."""
    if not slug:
        return None
    cand = brain_root() / "projects" / slug
    if (cand / ".METADATA.json").exists():
        return cand
    archive_dir = brain_root() / "archive"
    if archive_dir.exists():
        for meta in archive_dir.rglob(".METADATA.json"):
            if meta.parent.name == slug:
                return meta.parent
    return None


def record_deletion_in_project(slug: str, task: dict, deleted_date: str) -> None:
    proj = find_project_dir(slug)
    if proj is None:
        return
    meta_path = proj / ".METADATA.json"
    try:
        meta = read_json(meta_path)
    except (OSError, ValueError):
        return
    entry = {
        "task_id": task.get("task_id"),
        "task_name": task.get("task_name"),
        "backlogged_date": task.get("backlogged_date"),
        "deleted_date": deleted_date,
    }
    meta.setdefault("deleted_backlog_tasks", []).append(entry)
    if isinstance(meta.get("tasks"), list) and task.get("task_id") in meta["tasks"]:
        meta["tasks"].remove(task.get("task_id"))
    write_json(meta_path, meta)

    notes = proj / "notes.md"
    header = "## Deleted backlog tasks\n"
    line = (f"- **{task.get('task_id')}** {task.get('task_name')} — backlogged "
            f"{task.get('backlogged_date')}, auto-deleted {deleted_date} "
            f"(>6mo in backlog). Restore from git history if needed.\n")
    try:
        existing = notes.read_text() if notes.exists() else ""
        if header not in existing:
            existing += ("\n" if existing and not existing.endswith("\n") else "") + "\n" + header
        existing += line
        notes.write_text(existing)
    except OSError:
        pass


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--report", action="store_true", help="print a JSON summary (debug)")
    p.add_argument("--dry-run", action="store_true", help="report only, change nothing")
    args = p.parse_args()

    today = date.today()
    cutoff = minus_six_months(today)  # backlogged strictly before this => >6mo old
    path = tasks_csv()
    cols, rows = read_csv(path)

    deleted, kept = [], []
    for r in rows:
        if (r.get("status") or "").strip() == "backlog":
            bd = parse_date(r.get("backlogged_date"))
            if bd is not None and bd < cutoff:
                deleted.append(r)
                continue
        kept.append(r)

    if args.dry_run:
        print(json.dumps({
            "cutoff": cutoff.isoformat(),
            "would_delete": [{"task_id": d.get("task_id"),
                              "task_name": d.get("task_name"),
                              "backlogged_date": d.get("backlogged_date"),
                              "project": d.get("project")} for d in deleted],
        }, indent=2, ensure_ascii=False))
        return 0

    if deleted:
        for d in deleted:
            proj = (d.get("project") or "").strip()
            if proj:
                record_deletion_in_project(proj, d, today.isoformat())
        write_csv(path, cols, kept)

    if args.report:
        print(json.dumps({"deleted_count": len(deleted),
                          "deleted_ids": [d.get("task_id") for d in deleted]},
                         ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
