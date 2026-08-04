#!/usr/bin/env python3
"""Track whether this month's weekly triage has run yet.

'Monthly triage' is not a separate command — it's the FIRST weekly triage
of a calendar month. This script owns that one bit of state so /triage can
ask "is today's weekly triage also the monthly one?".

State file: <selected-workspace>/tasks/.monthly_triage.json
    {"last_monthly_triage_month": "YYYY-MM"}

Usage:
    monthly_triage_state.py           # check: prints JSON {is_monthly, month, last_recorded}
    monthly_triage_state.py --mark    # record current month as done; prints the new state

Flow in /triage weekly: run a plain check first; if is_monthly is true, do
the monthly backlog-review extras, THEN call --mark so the next weekly
triage this month is just weekly.
"""
import argparse
import json
import sys
from datetime import date

from _csvlib import brain_root


def state_path():
    return brain_root() / "tasks" / ".monthly_triage.json"


def _load() -> str:
    try:
        return json.loads(state_path().read_text()).get("last_monthly_triage_month", "")
    except (OSError, ValueError):
        return ""


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--mark", action="store_true",
                   help="record the current month as having had its monthly triage")
    args = p.parse_args()

    month = date.today().strftime("%Y-%m")
    last = _load()
    is_monthly = (last != month)

    if args.mark:
        state_path().write_text(json.dumps({"last_monthly_triage_month": month}) + "\n")
        print(json.dumps({"marked": month, "previous": last}))
        return 0

    print(json.dumps({"is_monthly": is_monthly, "month": month, "last_recorded": last}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
