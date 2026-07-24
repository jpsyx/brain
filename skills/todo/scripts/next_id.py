#!/usr/bin/env python3
"""Issue a new short ID for a task or habit.

IDs are prefix + integer:
- Tasks (in tasks.csv) use `T1, T2, T3, ...`
- Habits (in habits.csv) use `H1, H2, H3, ...`

Counters live next to the CSVs, one plaintext file per kind:
- ~/brain/tasks/.tasks_next_id   (next T# to issue)
- ~/brain/tasks/.habits_next_id  (next H# to issue)

Each counter file holds a single decimal integer with no other content.
Reading + incrementing is not atomic across concurrent processes; this is
a single-user system so that's fine. If a counter file is missing it is
recreated from the highest existing ID in the corresponding CSV.

Usage:
    next_id.py --kind tasks      # prints e.g. "T108"
    next_id.py --kind habits     # prints e.g. "H42"
    next_id.py --peek --kind tasks   # show next ID without consuming it

Importable:
    from next_id import new_id
    tid = new_id("tasks")   # e.g. "T108"
    hid = new_id("habits")  # e.g. "H42"
"""
import argparse
import csv
import re
import sys
from pathlib import Path

BRAIN = Path.home() / "brain"
TASKS_DIR = BRAIN / "tasks"

KINDS = {
    "tasks": {
        "prefix": "T",
        "counter": TASKS_DIR / ".tasks_next_id",
        "csv": TASKS_DIR / "tasks.csv",
    },
    "habits": {
        "prefix": "H",
        "counter": TASKS_DIR / ".habits_next_id",
        "csv": TASKS_DIR / "habits.csv",
    },
}


def _max_existing(csv_path: Path, prefix: str) -> int:
    """Largest <prefix>N integer found in the CSV's task_id column, or 0."""
    if not csv_path.exists():
        return 0
    pat = re.compile(rf"^{re.escape(prefix)}(\d+)$")
    hi = 0
    with open(csv_path, newline="") as f:
        for r in csv.DictReader(f):
            m = pat.match((r.get("task_id") or "").strip())
            if m:
                hi = max(hi, int(m.group(1)))
    return hi


def _read_counter(kind: str) -> int:
    cfg = KINDS[kind]
    cf: Path = cfg["counter"]
    if cf.exists():
        txt = cf.read_text().strip()
        if txt.isdigit():
            return int(txt)
    # rebuild from CSV if counter is missing/corrupt
    return _max_existing(cfg["csv"], cfg["prefix"]) + 1


def _write_counter(kind: str, value: int) -> None:
    KINDS[kind]["counter"].write_text(f"{value}\n")


def peek(kind: str) -> str:
    return f"{KINDS[kind]['prefix']}{_read_counter(kind)}"


def new_id(kind: str) -> str:
    n = _read_counter(kind)
    _write_counter(kind, n + 1)
    return f"{KINDS[kind]['prefix']}{n}"


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--kind", required=True, choices=sorted(KINDS))
    p.add_argument("--peek", action="store_true",
                   help="show next ID without consuming the counter")
    args = p.parse_args()
    print(peek(args.kind) if args.peek else new_id(args.kind))
    return 0


if __name__ == "__main__":
    sys.exit(main())
