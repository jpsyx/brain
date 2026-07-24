"""Shared helpers for tasks.csv / habits.csv mutators.

Keeps add_task / defer_task / mark_done DRY. Not a CLI; imported only.
"""
import csv
import re
import sys
from datetime import date, datetime, timedelta
from pathlib import Path

from next_id import new_id

BRAIN = Path.home() / "brain"
TASKS_CSV = BRAIN / "tasks" / "tasks.csv"
HABITS_CSV = BRAIN / "tasks" / "habits.csv"

# T### lives in tasks.csv, H### lives in habits.csv.
TASK_ID_RE = re.compile(r"^[Tt](\d+)$")
HABIT_ID_RE = re.compile(r"^[Hh](\d+)$")
BARE_INT_RE = re.compile(r"^\d+$")

# Chunked task naming convention: "<base> (<i>/<N>)". See SKILL.md
# "Chunked tasks" for the lifecycle. Anchored to end-of-string so a name
# like "Pay (Q1) bills (2/5)" still parses correctly.
CHUNK_NAME_RE = re.compile(r"^(.+) \((\d+)/(\d+)\)$")


def today_iso() -> str:
    return date.today().isoformat()


def new_task_id() -> str:
    """Issue a new tasks.csv ID (T###)."""
    return new_id("tasks")


def new_habit_id() -> str:
    """Issue a new habits.csv ID (H###)."""
    return new_id("habits")


def parse_date(s: str):
    s = (s or "").strip()
    if not s:
        return None
    return datetime.fromisoformat(s.split("T")[0]).date()


def shift_due(due_str: str, delta_days: int) -> str:
    d = parse_date(due_str) or date.today()
    return (d + timedelta(days=delta_days)).isoformat()


def touch_row(row: dict) -> None:
    """Set last_touched to today on this row. Safe to call on habit rows too —
    write_csv filters keys not in the file's columns, so the no-op is silent."""
    row["last_touched"] = today_iso()


def parse_chunk_name(name: str):
    """Return (base, index, total) if `name` matches the chunked-task naming
    convention "<base> (<i>/<N>)", else None. Anchored on the trailing
    parenthesized fraction, so any "(i/N)" earlier in the name is ignored.
    """
    m = CHUNK_NAME_RE.match((name or "").strip())
    if not m:
        return None
    return m.group(1), int(m.group(2)), int(m.group(3))


def find_next_chunk(rows, current_row):
    """Given a row that is part of a chunk family, return (idx, row) for the
    immediate next chunk in the same family, or (None, None) if there is no
    next chunk. "Next" means: same base name, same total N, index = current+1.
    """
    parsed = parse_chunk_name(current_row.get("task_name") or "")
    if not parsed:
        return None, None
    base, i, n = parsed
    if i >= n:
        return None, None
    target = f"{base} ({i + 1}/{n})"
    for j, r in enumerate(rows):
        if (r.get("task_name") or "").strip() == target:
            return j, r
    return None, None


def chunks_after(rows, current_row):
    """Return [(chunk_index, row_index, row), ...] for every chunk in the same
    family as `current_row` whose chunk index is strictly greater than
    current_row's, sorted by chunk index ascending. Returns [] if current_row
    isn't a chunk or has no later siblings present.
    """
    parsed = parse_chunk_name(current_row.get("task_name") or "")
    if not parsed:
        return []
    base, current_i, n = parsed
    later = []
    for j, r in enumerate(rows):
        p = parse_chunk_name(r.get("task_name") or "")
        if not p:
            continue
        rb, ri, rn = p
        if rb == base and rn == n and ri > current_i:
            later.append((ri, j, r))
    later.sort(key=lambda x: x[0])
    return later


def cascade_chunk_dates_forward(rows, current_row):
    """Ensure later chunks in `current_row`'s family have `due_date` >=
    current_row's `due_date`. Pushes later chunks forward only when their
    current `due_date` would otherwise invert the family order; never pulls
    them backward and never bumps `defer_count` (the defer of current_row is
    the caller's responsibility). Calls touch_row on each modified row.

    Returns the list of rows that were pushed, in chunk-index order. Returns
    [] if current_row isn't a chunk or no cascade was needed.
    """
    anchor = parse_date(current_row.get("due_date") or "")
    if anchor is None:
        return []
    later = chunks_after(rows, current_row)
    if not later:
        return []
    pushed = []
    floor = anchor
    for _, row_idx, family_row in later:
        current_due = parse_date(family_row.get("due_date") or "")
        if current_due is None or current_due < floor:
            rows[row_idx]["due_date"] = floor.isoformat()
            touch_row(rows[row_idx])
            pushed.append(rows[row_idx])
        else:
            floor = current_due
    return pushed


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


def _normalize_id(needle: str):
    """Return a canonical ID (e.g. 'T17', 'H42') if `needle` is recognizable
    as an ID, else None. Bare integers return None — they must be resolved
    against both CSVs by the caller (see locate())."""
    n = needle.strip()
    m = TASK_ID_RE.match(n)
    if m:
        return f"T{int(m.group(1))}"
    m = HABIT_ID_RE.match(n)
    if m:
        return f"H{int(m.group(1))}"
    return None


def find_by_id_or_fuzzy(rows, needle: str):
    """Return (idx, row) or (None, None).

    Match order:
    1. Exact task_id match (case-insensitive on the T/H prefix; e.g. 't17' == 'T17').
    2. Bare integer ('17') matches a row whose task_id ends in that integer
       (e.g. T17, H17). Caller (locate) is responsible for handling
       cross-CSV collisions when called with just a bare integer.
    3. Case-insensitive substring match against task_name.
       Errors if multiple fuzzy hits.
    """
    n = needle.strip()
    canonical = _normalize_id(n)
    if canonical:
        for i, r in enumerate(rows):
            if (r.get("task_id") or "").strip() == canonical:
                return i, r
        return None, None
    if BARE_INT_RE.match(n):
        # match any row whose ID equals T<n> or H<n>
        suffixed = {f"T{int(n)}", f"H{int(n)}"}
        for i, r in enumerate(rows):
            if (r.get("task_id") or "").strip() in suffixed:
                return i, r
        return None, None
    low = n.lower()
    hits = [(i, r) for i, r in enumerate(rows) if low in (r.get("task_name") or "").lower()]
    if len(hits) == 1:
        return hits[0]
    if len(hits) > 1:
        print(f"ambiguous: {len(hits)} tasks match '{needle}':", file=sys.stderr)
        for _, r in hits:
            print(f"  - {r.get('task_id')} {r.get('task_name')}", file=sys.stderr)
        sys.exit(2)
    return None, None


def locate(needle: str):
    """Search both tasks.csv and habits.csv. Returns (path, columns, rows, idx, row).

    If `needle` is a bare integer and both T<n> and H<n> exist, errors out
    asking the user to disambiguate with the prefix.
    """
    n = needle.strip()
    if BARE_INT_RE.match(n):
        hits = []
        for path in (TASKS_CSV, HABITS_CSV):
            cols, rows = read_csv(path)
            idx, row = find_by_id_or_fuzzy(rows, n)
            if row is not None:
                hits.append((path, cols, rows, idx, row))
        if len(hits) > 1:
            print(
                f"ambiguous: bare ID '{n}' matches both "
                f"T{int(n)} (tasks.csv) and H{int(n)} (habits.csv). "
                f"Use the prefix: 'T{int(n)}' or 'H{int(n)}'.",
                file=sys.stderr,
            )
            sys.exit(2)
        if hits:
            return hits[0]
        print(f"no task matched '{needle}'", file=sys.stderr)
        sys.exit(1)

    for path in (TASKS_CSV, HABITS_CSV):
        cols, rows = read_csv(path)
        idx, row = find_by_id_or_fuzzy(rows, n)
        if row is not None:
            return path, cols, rows, idx, row
    print(f"no task matched '{needle}'", file=sys.stderr)
    sys.exit(1)
