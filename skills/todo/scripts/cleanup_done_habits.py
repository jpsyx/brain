#!/usr/bin/env python3
"""Delete habits.csv rows where status=done AND completed_date <= today - 7d.

Run as part of `/second-brain reindex` and `/todo reindex` so habits.csv stays
short. The completed-but-recent rows stay for a week so the user can
inspect / undo.
"""
import sys
import json
from datetime import date, datetime, timedelta

from _csvlib import brain_root, habits_csv, read_csv, write_csv

CUTOFF = date.today() - timedelta(days=7)
MANAGED_TRIAGE_KEYS = {"brain.triage.daily", "brain.triage.weekly"}


def parse_date(s: str):
    s = (s or "").strip()
    if not s:
        return None
    return datetime.fromisoformat(s.split("T")[0]).date()


def triage_habits_enabled() -> bool:
    path = brain_root() / ".config" / "config.json"
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return True
    return value.get("enable_triage_habits", True) is not False


def main() -> int:
    path = habits_csv()
    if not path.exists():
        print(f"no {path}", file=sys.stderr)
        return 0
    columns, rows = read_csv(path)
    triage_enabled = triage_habits_enabled()

    keep, drop = [], []
    deferred_managed_purge = 0
    for r in rows:
        managed = (r.get("system_key") or "").strip() in MANAGED_TRIAGE_KEYS
        if managed:
            keep.append(r)
            if not triage_enabled:
                deferred_managed_purge += 1
            continue
        if r.get("status") == "done":
            cd = parse_date(r.get("completed_date") or "")
            if cd is not None and cd <= CUTOFF:
                drop.append(r)
                continue
        keep.append(r)

    if not drop:
        print(f"cleanup: no done habits older than {CUTOFF}; kept {len(keep)} rows")
        if deferred_managed_purge:
            print(
                "cleanup: managed triage purge is transactional; "
                "run `brain config set enable_triage_habits false`"
            )
        return 0

    write_csv(path, columns, keep)
    print(f"cleanup: dropped {len(drop)} done habit(s) older than {CUTOFF}; kept {len(keep)} rows")
    for r in drop:
        print(f"  - {r.get('task_name')} (completed {r.get('completed_date')})")
    if deferred_managed_purge:
        print(
            "cleanup: managed triage purge is transactional; "
            "run `brain config set enable_triage_habits false`"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
