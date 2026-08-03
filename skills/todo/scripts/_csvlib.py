"""Shared helpers for tasks.csv / habits.csv mutators.

Keeps task/habit mutator scripts DRY. Not a CLI; imported only.
"""
from __future__ import annotations

import csv
import json
import os
import re
import sqlite3
import sys
import tempfile
import uuid
from datetime import date, datetime, timedelta
from pathlib import Path

# T### lives in tasks.csv, H### lives in habits.csv.
TASK_ID_RE = re.compile(r"^[Tt](\d+)$")
HABIT_ID_RE = re.compile(r"^[Hh](\d+)$")
BARE_INT_RE = re.compile(r"^\d+$")

# Chunked task naming convention: "<base> (<i>/<N>)". See SKILL.md
# "Chunked tasks" for the lifecycle. Anchored to end-of-string so a name
# like "Pay (Q1) bills (2/5)" still parses correctly.
CHUNK_NAME_RE = re.compile(r"^(.+) \((\d+)/(\d+)\)$")
USER_ID_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
MANAGED_TRIAGE_KEYS = {"brain.triage.daily", "brain.triage.weekly"}
_READ_SNAPSHOTS: dict[Path, bytes | None] = {}


def _required_environment(name: str) -> str:
    value = (os.environ.get(name) or "").strip()
    if not value:
        raise SystemExit(f"{name} is required; launch this script through Brain")
    return value


def brain_root() -> Path:
    root = Path(_required_environment("BRAIN_ROOT"))
    if not root.is_absolute():
        raise SystemExit("BRAIN_ROOT must be absolute; launch this script through Brain")
    return root


def actor_id() -> str:
    return _required_environment("BRAIN_ACTOR_ID")


def tasks_csv() -> Path:
    return brain_root() / "tasks" / "tasks.csv"


def habits_csv() -> Path:
    return brain_root() / "tasks" / "habits.csv"


def new_uuid() -> str:
    """Create an immutable UUID for callers that own a UUID-bearing schema."""
    return str(uuid.uuid4())


def validate_assigned_to(user_id: str) -> str:
    """Return an explicit assignment only when it names a portable member."""
    if not USER_ID_RE.fullmatch(user_id):
        raise SystemExit(
            f"invalid assigned_to '{user_id}'; use a lower-case kebab user ID"
        )
    path = brain_root() / ".config" / "users.json"
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        raise SystemExit(f"cannot validate assigned_to without {path}: {error}") from error
    except json.JSONDecodeError as error:
        raise SystemExit(f"cannot validate assigned_to from {path}: {error}") from error
    members = {user.get("id") for user in data.get("users", [])}
    if user_id not in members:
        raise SystemExit(f"assigned_to '{user_id}' is not a workspace member")
    return user_id


def assignment_for_create(explicit: str | None) -> str:
    """Default assignment to the effective actor; validate explicit changes."""
    return actor_id() if explicit is None else validate_assigned_to(explicit)


def today_iso() -> str:
    return date.today().isoformat()


def new_task_id() -> str:
    """Issue a new tasks.csv ID (T###)."""
    from next_id import new_id

    return new_id("tasks")


def new_habit_id() -> str:
    """Issue a new habits.csv ID (H###)."""
    from next_id import new_id

    return new_id("habits")


def parse_date(s: str):
    s = (s or "").strip()
    if not s:
        return None
    return datetime.fromisoformat(s.split("T")[0]).date()


def shift_due(due_str: str, delta_days: int) -> str:
    d = parse_date(due_str) or date.today()
    return (d + timedelta(days=delta_days)).isoformat()


def touch_row(row: dict, touched: str | None = None) -> None:
    """Set last_touched to today on this row."""
    row["last_touched"] = touched or today_iso()


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
        _READ_SNAPSHOTS[path] = None
        return [], []
    _READ_SNAPSHOTS[path] = path.read_bytes()
    with open(path, newline="") as f:
        reader = csv.DictReader(f)
        return _canonical_assignment(reader.fieldnames or [], list(reader))


def write_csv(path: Path, columns, rows):
    columns, rows = _canonical_assignment(columns, rows)
    if (
        "task_uuid" not in columns
        and any((r.get("task_uuid") or "").strip() for r in rows)
    ):
        # Keep task_id first until the coordinated migration switches merge
        # identity. Fresh/current schemas already declare task_uuid first.
        columns = [*columns, "task_uuid"]
    if any((r.get("last_touched") or "").strip() for r in rows) and "last_touched" not in columns:
        columns = list(columns) + ["last_touched"]
    lock = _acquire_task_store_lock()
    try:
        _reject_pending_rust_transaction()
        before = _READ_SNAPSHOTS.get(path, object())
        current = path.read_bytes() if path.exists() else None
        if not isinstance(before, (bytes, type(None))) or before != current:
            raise SystemExit(f"{path} changed after it was read; retry the task operation")
        _protect_managed_removal(before, rows)
        path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(
            mode="w", newline="", dir=path.parent, prefix=f".{path.name}.",
            suffix=".pending", delete=False
        ) as f:
            temporary = Path(f.name)
            w = csv.DictWriter(f, fieldnames=columns, quoting=csv.QUOTE_MINIMAL)
            w.writeheader()
            for r in rows:
                w.writerow({c: r.get(c, "") for c in columns})
            f.flush()
            os.fsync(f.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
        _READ_SNAPSHOTS[path] = path.read_bytes()
    finally:
        lock.rollback()
        lock.close()


def _task_store_lock_path() -> Path:
    raw = _required_environment("BRAIN_WORKSPACE_ID")
    try:
        workspace_id = str(uuid.UUID(raw))
    except ValueError as error:
        raise SystemExit("BRAIN_WORKSPACE_ID must be a UUID") from error
    return Path.home() / ".cache" / "brain" / "workspaces" / workspace_id / "tasks.transaction.lock"


def _acquire_task_store_lock():
    path = _task_store_lock_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    connection = sqlite3.connect(path, timeout=30, isolation_level=None)
    connection.execute("PRAGMA journal_mode = OFF")
    connection.execute("BEGIN IMMEDIATE")
    return connection


def _reject_pending_rust_transaction() -> None:
    journal = brain_root() / ".config" / ".brain-triage-habits-transaction.json"
    if journal.exists():
        raise SystemExit(f"pending task transaction at {journal}; run Brain to recover it")


def _protect_managed_removal(before: bytes | None, after_rows) -> None:
    if before is None or not _triage_habits_enabled():
        return
    old_rows = list(csv.DictReader(before.decode("utf-8").splitlines()))
    new_identities = {_row_identity(row) for row in after_rows}
    for row in old_rows:
        if row.get("system_key") in MANAGED_TRIAGE_KEYS and _row_identity(row) not in new_identities:
            raise SystemExit("managed triage rows cannot be removed while enable_triage_habits is true")


def _row_identity(row) -> tuple[str, ...]:
    task_uuid = (row.get("task_uuid") or "").strip()
    if task_uuid:
        return "uuid", task_uuid
    return (
        "display",
        (row.get("task_id") or "").strip(),
        (row.get("system_key") or "").strip(),
    )


def _triage_habits_enabled() -> bool:
    path = brain_root() / ".config" / "config.json"
    if not path.exists():
        return True
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"cannot read portable config {path}: {error}") from error
    if not isinstance(value, dict):
        raise SystemExit(f"portable config {path} must contain a JSON object")
    enabled = value.get("enable_triage_habits", True)
    if not isinstance(enabled, bool):
        raise SystemExit("enable_triage_habits must be true or false")
    return enabled


def _canonical_assignment(columns, rows):
    columns = list(columns)
    if "assigned_to" in columns:
        if "assignee" in columns:
            columns.remove("assignee")
            for row in rows:
                row.pop("assignee", None)
    elif "assignee" in columns:
        columns[columns.index("assignee")] = "assigned_to"
        for row in rows:
            row["assigned_to"] = row.pop("assignee", "")
    elif columns:
        columns.append("assigned_to")
        for row in rows:
            row.setdefault("assigned_to", "")
    return columns, rows


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
        for path in (tasks_csv(), habits_csv()):
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

    for path in (tasks_csv(), habits_csv()):
        cols, rows = read_csv(path)
        idx, row = find_by_id_or_fuzzy(rows, n)
        if row is not None:
            return path, cols, rows, idx, row
    print(f"no task matched '{needle}'", file=sys.stderr)
    sys.exit(1)
