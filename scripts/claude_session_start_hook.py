#!/usr/bin/env python3
"""Claude Code `SessionStart` hook for the `brain` two-panel shell.

Wired into `~/brain/.claude/settings.json` so it fires whenever a Claude
session in `~/brain` starts, resumes, clears (`/new` / `/clear`), or
compacts. It records the *current* session id for the launching `brain`
shell so the next `brain` startup can resume the right conversation —
including a fresh session created mid-run by `/new`.

`brain` passes three env vars down into the claude child it spawns:

  BRAIN_INSTANCE_ID — the brain shell's lineage id (one per running shell)
  BRAIN_PID         — the brain shell's PID (the session's lock owner)
  BRAIN_STATE_DB    — path to the shared SQLite state DB

If BRAIN_STATE_DB / BRAIN_INSTANCE_ID are unset, this is an "ambient"
claude run (the user opened claude in ~/brain directly, not via the brain
panel) and the hook is a no-op.

The hook upserts the session row (locked to BRAIN_PID, marked active now)
and frees any *other* session this instance held — so exactly one session
is current per shell, and the pre-`/new` conversation stays resumable
later via the lock+recency model in `brain`'s `state` module.

Failure modes are deliberately silent — the hook MUST NOT raise. A crash
inside claude is loud and would distract from the session. We swallow
everything and exit 0.
"""
from __future__ import annotations

import json
import os
import sqlite3
import sys
import time


def main() -> None:
    db_path = os.environ.get("BRAIN_STATE_DB")
    instance = os.environ.get("BRAIN_INSTANCE_ID")
    if not db_path or not instance:
        return  # ambient claude usage — not a brain panel

    try:
        data = json.load(sys.stdin)
    except Exception:
        return
    session_id = data.get("session_id")
    if not session_id:
        return
    source = data.get("source")

    pid_raw = os.environ.get("BRAIN_PID", "")
    pid = int(pid_raw) if pid_raw.isdigit() else None
    now = int(time.time())

    try:
        conn = sqlite3.connect(db_path, timeout=5)
    except Exception:
        return
    try:
        conn.execute("PRAGMA busy_timeout = 5000;")
        # Upsert the current session, locked to this brain shell.
        conn.execute(
            """
            INSERT INTO brain_sessions
              (claude_session_id, brain_instance_id, locked_pid, source,
               created_at, last_active_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(claude_session_id) DO UPDATE SET
              brain_instance_id = excluded.brain_instance_id,
              locked_pid        = excluded.locked_pid,
              source            = excluded.source,
              last_active_at    = excluded.last_active_at
            """,
            (session_id, instance, pid, source, now, now),
        )
        # Exactly one current session per instance: free the others so a
        # /new (which may rotate the id) leaves the prior one resumable.
        conn.execute(
            """
            UPDATE brain_sessions SET locked_pid = NULL
            WHERE brain_instance_id = ? AND claude_session_id <> ?
            """,
            (instance, session_id),
        )
        conn.commit()
    except Exception:
        pass
    finally:
        conn.close()


if __name__ == "__main__":
    try:
        main()
    except Exception:
        pass
    sys.exit(0)
