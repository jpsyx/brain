#!/usr/bin/env python3
"""Track consecutive nights of late evening work for the /todo agenda.

A "late work night" is any day whose agenda schedules a work/coding/
work-flavored task between 7:01 PM and 9 PM. The /todo SKILL.md owns
the judgment of whether tonight qualifies; this script owns the
persistence and the calendar arithmetic (LLMs are unreliable at date
math, so streak counting lives here).

Storage: <selected-workspace>/tasks/.late_work_streak.json
Format:  {"late_nights": ["YYYY-MM-DD", ...]}  (sorted, deduplicated)

Subcommands:
- status [--date YYYY-MM-DD]
    Print the current streak as JSON. The streak is the length of the
    maximal consecutive run of late nights ending at `--date` (default:
    today) if `--date` is itself a late night, otherwise ending at the
    most recent late night strictly before `--date`. If the most recent
    late night is older than yesterday relative to `--date`, the streak
    is considered broken and reported as 0 (with `last_late_night` still
    populated so callers can show context).

- mark <YYYY-MM-DD>
    Record the date as a late work night. Idempotent. Prints the
    resulting streak (computed as of that date).

- unmark <YYYY-MM-DD>
    Remove the date if it was previously marked. Idempotent.
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import date, timedelta

from _csvlib import brain_root


def state_path():
    return brain_root() / "tasks" / ".late_work_streak.json"


def _load() -> list[str]:
    path = state_path()
    if not path.exists():
        return []
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return []
    nights = data.get("late_nights", [])
    return sorted({d for d in nights if _is_iso_date(d)})


def _save(nights: list[str]) -> None:
    path = state_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps({"late_nights": sorted(set(nights))}, indent=2) + "\n",
        encoding="utf-8",
    )


def _is_iso_date(s: str) -> bool:
    try:
        date.fromisoformat(s)
        return True
    except ValueError:
        return False


def _parse_date(s: str) -> date:
    return date.fromisoformat(s)


def _streak_ending(target: date, nights: set[str]) -> int:
    """Count consecutive late nights ending at or before `target`.

    Walks backward from `target` while each day is in `nights`. If
    `target` itself isn't a late night but `target - 1` is, anchor at
    `target - 1` (the user hasn't decided about tonight yet but
    yesterday's streak is still real).
    """
    if target.isoformat() in nights:
        cursor = target
    elif (target - timedelta(days=1)).isoformat() in nights:
        cursor = target - timedelta(days=1)
    else:
        return 0
    count = 0
    while cursor.isoformat() in nights:
        count += 1
        cursor -= timedelta(days=1)
    return count


def _cmd_status(args: argparse.Namespace) -> int:
    target = _parse_date(args.date) if args.date else date.today()
    nights = _load()
    nights_set = set(nights)
    streak = _streak_ending(target, nights_set)
    last_late = nights[-1] if nights else None
    print(
        json.dumps(
            {
                "target_date": target.isoformat(),
                "streak": streak,
                "last_late_night": last_late,
                "target_is_late_night": target.isoformat() in nights_set,
            },
            indent=2,
        )
    )
    return 0


def _cmd_mark(args: argparse.Namespace) -> int:
    target = _parse_date(args.date)
    nights = _load()
    if target.isoformat() not in nights:
        nights.append(target.isoformat())
        _save(nights)
    streak = _streak_ending(target, set(nights))
    print(json.dumps({"streak": streak, "date": target.isoformat()}, indent=2))
    return 0


def _cmd_unmark(args: argparse.Namespace) -> int:
    target = _parse_date(args.date)
    nights = [d for d in _load() if d != target.isoformat()]
    _save(nights)
    print(json.dumps({"date": target.isoformat(), "removed": True}, indent=2))
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_status = sub.add_parser("status", help="Print current streak as JSON.")
    p_status.add_argument("--date", help="Anchor date (YYYY-MM-DD). Defaults to today.")
    p_status.set_defaults(func=_cmd_status)

    p_mark = sub.add_parser("mark", help="Mark a date as a late-work night.")
    p_mark.add_argument("date", help="YYYY-MM-DD")
    p_mark.set_defaults(func=_cmd_mark)

    p_unmark = sub.add_parser("unmark", help="Remove a late-work-night mark.")
    p_unmark.add_argument("date", help="YYYY-MM-DD")
    p_unmark.set_defaults(func=_cmd_unmark)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
