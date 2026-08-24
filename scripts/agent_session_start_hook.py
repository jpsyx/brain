#!/usr/bin/env python3
"""Frontend-neutral session-start bridge for a selected Brain workspace.

Wired into `<brain-root>/.claude/settings.json` so it fires whenever a Claude
session in that selected workspace starts, resumes, clears (`/new` /
`/clear`), or compacts. It records the *current* session id for the launching
`brain` shell so the next launch of that workspace can resume the right
conversation, including a fresh session created mid-run by `/new`. It never
creates a session for an unregistered shell lineage.

`brain` passes the common child-integration identity variables:

  BRAIN_WORKSPACE_ID — immutable selected workspace UUID
  BRAIN_WORKSPACE    — selected canonical workspace name
  BRAIN_ROOT         — selected workspace root
  BRAIN_ACTOR_ID     — actor attributed to this launch
  BRAIN_CHANNEL: initiating channel retained by follow-up turns
  BRAIN_AGENT_KIND: agent frontend (`claude`, `codex`, or `opencode`)

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

The hook updates an exact registered tuple or rotates an already registered
live lineage to the frontend-reported id, then frees any other session this
instance held. Receiver tuples additionally require their exact durable
registration and live lock. Ambient, forged, or released workspace/session
tuples are ignored.

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
    if data.get("parent_session_id") or data.get("parentID") or data.get("parent_id"):
        return
    session_id = data.get("session_id")
    if not session_id and launch["BRAIN_AGENT_KIND"] == "codex":
        session_id = data.get("thread_id")
    if not session_id:
        return
    source = data.get("source")

    pid_raw = os.environ.get("BRAIN_PID", "")
    pid = int(pid_raw) if pid_raw.isdigit() else None
    now = int(time.time())

    try:
        conn = sqlite3.connect(db_path, timeout=5, isolation_level=None)
    except Exception:
        return
    try:
        conn.execute("PRAGMA busy_timeout = 5000;")
        conn.execute("BEGIN IMMEDIATE")
        scope = (
            launch["BRAIN_AGENT_KIND"],
            launch["BRAIN_WORKSPACE_ID"],
            launch["BRAIN_ACTOR_ID"],
            launch["BRAIN_CHANNEL"],
        )
        exact = conn.execute(
            """
            SELECT 1 FROM brain_sessions AS session
            WHERE session.agent_kind = ? AND session.agent_session_id = ?
              AND session.workspace_id = ? AND session.actor_id = ?
              AND session.channel = ? AND session.brain_instance_id = ?
              AND (
                session.channel = 'interactive'
                OR (
                  session.locked_pid IS NOT NULL
                  AND EXISTS (
                    SELECT 1 FROM receiver_session_registrations AS registration
                    WHERE registration.workspace_id = session.workspace_id
                      AND registration.agent_kind = session.agent_kind
                      AND registration.actor_id = session.actor_id
                      AND registration.channel = session.channel
                      AND registration.brain_instance_id = session.brain_instance_id
                  )
                )
              )
            """,
            (scope[0], session_id, scope[1], scope[2], scope[3], instance),
        ).fetchone()
        if not exact:
            lineage = conn.execute(
                """
                SELECT 1 FROM brain_sessions
                WHERE agent_kind = ? AND workspace_id = ? AND actor_id = ?
                  AND channel = ? AND brain_instance_id = ?
                  AND locked_pid IS NOT NULL
                LIMIT 1
                """,
                (*scope, instance),
            ).fetchone()
            if not lineage:
                conn.rollback()
                return
            target = conn.execute(
                """
                SELECT brain_instance_id, locked_pid FROM brain_sessions
                WHERE agent_kind = ? AND agent_session_id = ? AND workspace_id = ?
                  AND actor_id = ? AND channel = ?
                """,
                (scope[0], session_id, scope[1], scope[2], scope[3]),
            ).fetchone()
            if target and target[0] != instance and target[1] is not None:
                conn.rollback()
                return

        conn.execute(
            """
            INSERT INTO brain_sessions
              (agent_kind, agent_session_id, brain_instance_id, locked_pid, source,
               workspace_id, actor_id, channel, created_at, last_active_at,
               completion_status)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active')
            ON CONFLICT(agent_kind, agent_session_id, workspace_id, actor_id, channel)
            DO UPDATE SET
              brain_instance_id = excluded.brain_instance_id,
              locked_pid        = excluded.locked_pid,
              source            = excluded.source,
              last_active_at    = excluded.last_active_at,
              completion_status = 'active'
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
        try:
            if conn.in_transaction:
                conn.rollback()
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
