#!/usr/bin/env python3
"""Claude Code `SessionStart` hook for a selected Brain workspace.

Wired into `<brain-root>/.claude/settings.json` so it fires whenever a Claude
session in that selected workspace starts, resumes, clears (`/new` /
`/clear`), or compacts. It records the *current* session id for the launching
`brain` shell so the next launch of that workspace can resume the right
conversation, including a fresh session created mid-run by `/new`.

`brain` passes the common child-integration identity variables:

  BRAIN_WORKSPACE_ID — immutable selected workspace UUID
  BRAIN_WORKSPACE    — selected canonical workspace name
  BRAIN_ROOT         — selected workspace root
  BRAIN_ACTOR_ID     — actor attributed to this launch
  BRAIN_CHANNEL: initiating channel retained by follow-up turns
  BRAIN_AGENT_KIND: agent frontend (`claude` or `codex`)

The agent session extends that environment with:

  BRAIN_INSTANCE_ID — the brain shell's lineage id (one per running shell)
  BRAIN_PID         — the brain shell's PID (the session's lock owner)
  BRAIN_STATE_DB    — selected workspace's UUID-scoped SQLite state DB
  BRAIN_RESPONSE_DIR — selected workspace's UUID-scoped response directory

If any common identity variable, BRAIN_STATE_DB, or BRAIN_INSTANCE_ID is
unset, this is not a fully attributed Brain panel launch and the hook is a
no-op. BRAIN_PID remains optional because an invalid or absent PID can safely
produce an unlocked session row. BRAIN_RESPONSE_DIR is consumed by the Stop
hook rather than this SessionStart hook.

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
    required = (
        "BRAIN_WORKSPACE_ID",
        "BRAIN_WORKSPACE",
        "BRAIN_ROOT",
        "BRAIN_ACTOR_ID",
        "BRAIN_CHANNEL",
        "BRAIN_AGENT_KIND",
        "BRAIN_INSTANCE_ID",
        "BRAIN_STATE_DB",
    )
    launch = {name: os.environ.get(name) for name in required}
    if not all(launch.values()):
        return
    db_path = launch["BRAIN_STATE_DB"]
    instance = launch["BRAIN_INSTANCE_ID"]

    try:
        data = json.load(sys.stdin)
    except Exception:
        return
    session_id = data.get("session_id") or data.get("thread_id")
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
              (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
               workspace_id, actor_id, channel, created_at, last_active_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(agent_kind, agent_session_id, workspace_id, actor_id, channel)
            DO UPDATE SET
              brain_instance_id = excluded.brain_instance_id,
              locked_pid        = excluded.locked_pid,
              source            = excluded.source,
              last_active_at    = excluded.last_active_at
            """,
            (
                launch["BRAIN_AGENT_KIND"],
                session_id,
                instance,
                pid,
                source,
                launch["BRAIN_WORKSPACE_ID"],
                launch["BRAIN_ACTOR_ID"],
                launch["BRAIN_CHANNEL"],
                now,
                now,
            ),
        )
        # Exactly one current session per instance: free the others so a
        # /new (which may rotate the id) leaves the prior one resumable.
        conn.execute(
            """
            UPDATE brain_sessions SET locked_pid = NULL
            WHERE brain_instance_id = ?
              AND NOT (
                agent_kind = ? AND agent_session_id = ? AND workspace_id = ?
                AND actor_id = ? AND channel = ?
              )
            """,
            (
                instance,
                launch["BRAIN_AGENT_KIND"],
                session_id,
                launch["BRAIN_WORKSPACE_ID"],
                launch["BRAIN_ACTOR_ID"],
                launch["BRAIN_CHANNEL"],
            ),
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
