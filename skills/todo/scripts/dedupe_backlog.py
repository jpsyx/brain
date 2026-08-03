#!/usr/bin/env python3
"""Silently delete backlog tasks that an active task has already superseded.

Monthly-triage phase. If a `backlog` task has a near-identical twin on the
ACTIVE list (status not in done/backlog) AND that active twin was
`created_date` AFTER the backlog task's `backlogged_date`, then the user
deliberately re-created the task at some point after parking it — i.e. they
already chose to "revive" it by hand. The backlog copy is now a stale
duplicate, so we just delete it. Nothing else: no revive bookkeeping, no
project breadcrumb, no output.

Why the date guard matters: if the active twin was created BEFORE the
backlog task was parked, the two merely coexisted (or the active one is
unrelated) — that is NOT an intentional re-creation, so we leave the
backlog task alone.

Duplicate match is conservative and deterministic: task names are equal
after normalizing (lowercase, trim, collapse internal whitespace, strip
surrounding punctuation). Reworded titles won't match — that's intended;
a false delete is worse than leaving a near-dup for the agent to catch.

DELIBERATELY SILENT (like purge_old_backlog.py): prints nothing unless
`--report`/`--dry-run`. /triage runs it plain.

Usage:
    dedupe_backlog.py            # silent dedupe (monthly triage use)
    dedupe_backlog.py --dry-run  # show pairs that WOULD be deleted, change nothing
    dedupe_backlog.py --report   # dedupe + print a JSON summary (debug)
"""
import argparse
import json
import re
import sys

from _csvlib import parse_date, read_csv, tasks_csv, write_csv

_PUNCT = re.compile(r"[^\w\s]")
_WS = re.compile(r"\s+")


def normalize(name: str) -> str:
    n = (name or "").strip().lower()
    n = _PUNCT.sub("", n)
    n = _WS.sub(" ", n).strip()
    return n


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("--report", action="store_true")
    args = p.parse_args()

    path = tasks_csv()
    cols, rows = read_csv(path)

    # Index active (non-done, non-backlog) tasks by normalized name -> latest created_date.
    active_created = {}
    for r in rows:
        st = (r.get("status") or "").strip()
        if st in ("done", "backlog"):
            continue
        key = normalize(r.get("task_name"))
        cd = parse_date(r.get("created_date"))
        if not key or cd is None:
            continue
        if key not in active_created or cd > active_created[key]:
            active_created[key] = cd

    to_delete = []
    for r in rows:
        if (r.get("status") or "").strip() != "backlog":
            continue
        key = normalize(r.get("task_name"))
        bd = parse_date(r.get("backlogged_date"))
        twin_created = active_created.get(key)
        # Delete only when an active twin exists AND it was created strictly
        # after this task was backlogged (the intentional re-creation case).
        if twin_created is not None and bd is not None and twin_created > bd:
            to_delete.append(r)

    del_ids = {r.get("task_id") for r in to_delete}

    if args.dry_run:
        print(json.dumps({"would_delete": [
            {"task_id": r.get("task_id"), "task_name": r.get("task_name"),
             "backlogged_date": r.get("backlogged_date")} for r in to_delete]},
            indent=2, ensure_ascii=False))
        return 0

    if to_delete:
        kept = [r for r in rows if r.get("task_id") not in del_ids]
        write_csv(path, cols, kept)

    if args.report:
        print(json.dumps({"deleted_count": len(to_delete),
                          "deleted_ids": sorted(del_ids)}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
