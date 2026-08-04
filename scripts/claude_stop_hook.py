#!/usr/bin/env python3
"""Record Claude's final assistant message for an inbound brain job."""
from __future__ import annotations

import json
import os
import pathlib
import sqlite3
import sys


def main() -> None:
    required = (
        "BRAIN_RESPONSE_DIR",
        "BRAIN_STATE_DB",
        "BRAIN_WORKSPACE_ID",
        "BRAIN_AGENT_KIND",
        "BRAIN_INSTANCE_ID",
        "BRAIN_ACTOR_ID",
        "BRAIN_CHANNEL",
    )
    launch = {name: os.environ.get(name) for name in required}
    if not all(launch.values()):
        return
    try:
        payload = json.load(sys.stdin)
        session_id = payload.get("session_id") or payload.get("thread_id")
        response_id = os.environ.get("BRAIN_RESPONSE_ID") or session_id
        message = payload.get("last_assistant_message")
        if not session_id or not response_id or not isinstance(message, str) or not message.strip():
            return
        conn = sqlite3.connect(launch["BRAIN_STATE_DB"], timeout=5)
        try:
            registered = conn.execute(
                """
                SELECT 1 FROM brain_sessions
                WHERE agent_kind = ? AND agent_session_id = ? AND workspace_id = ?
                  AND actor_id = ? AND channel = ? AND brain_instance_id = ?
                """,
                (
                    launch["BRAIN_AGENT_KIND"],
                    session_id,
                    launch["BRAIN_WORKSPACE_ID"],
                    launch["BRAIN_ACTOR_ID"],
                    launch["BRAIN_CHANNEL"],
                    launch["BRAIN_INSTANCE_ID"],
                ),
            ).fetchone()
            if not registered:
                return
            conn.execute(
                """
                UPDATE brain_sessions SET completion_status = 'completed'
                WHERE agent_kind = ? AND agent_session_id = ? AND workspace_id = ?
                  AND actor_id = ? AND channel = ?
                """,
                (
                    launch["BRAIN_AGENT_KIND"],
                    session_id,
                    launch["BRAIN_WORKSPACE_ID"],
                    launch["BRAIN_ACTOR_ID"],
                    launch["BRAIN_CHANNEL"],
                ),
            )
            conn.commit()
        finally:
            conn.close()
        target_dir = pathlib.Path(launch["BRAIN_RESPONSE_DIR"])
        target_dir.mkdir(parents=True, exist_ok=True)
        target = target_dir / f"{response_id}.json"
        temporary = target.with_suffix(".tmp")
        temporary.write_text(
            json.dumps({
                "session_id": session_id,
                "response_id": response_id,
                "frontend": launch["BRAIN_AGENT_KIND"],
                "workspace_id": launch["BRAIN_WORKSPACE_ID"],
                "actor_id": launch["BRAIN_ACTOR_ID"],
                "channel": launch["BRAIN_CHANNEL"],
                "completion_status": "completed",
                "message": message,
            }),
            encoding="utf-8",
        )
        temporary.replace(target)
    except Exception:
        return


if __name__ == "__main__":
    main()
