#!/usr/bin/env python3
"""Record a frontend's final assistant message when its session stops."""
from __future__ import annotations

import json
import os
import pathlib
import sqlite3
import sys
import tempfile


def assistant_text(content: object) -> str:
    """Join the text blocks of one assistant message's content."""
    if isinstance(content, str):
        return content
    if not isinstance(content, list):
        return ""
    parts = []
    for block in content:
        if isinstance(block, dict) and block.get("type") == "text":
            value = block.get("text")
            if isinstance(value, str) and value:
                parts.append(value)
    return "\n\n".join(parts)


def transcript_final_message(transcript_path: object) -> str | None:
    """Read the last assistant text message from a Claude transcript JSONL.

    Each line is one event; assistant turns carry
    `{"type": "assistant", "message": {"role": "assistant", "content": [...]}}`
    with `text` blocks holding the spoken reply. Returns the last such message
    that has any text, or None when the transcript is unreadable or has none.
    """
    if not isinstance(transcript_path, str) or not transcript_path:
        return None
    try:
        with open(transcript_path, encoding="utf-8") as handle:
            lines = handle.readlines()
    except OSError:
        return None
    latest = None
    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            record = json.loads(line)
        except ValueError:
            continue
        if not isinstance(record, dict) or record.get("type") != "assistant":
            continue
        message = record.get("message")
        if not isinstance(message, dict) or message.get("role") != "assistant":
            continue
        text = assistant_text(message.get("content"))
        if text.strip():
            latest = text
    return latest


def resolve_final_message(payload: dict, agent_kind: str) -> str | None:
    """The turn's final assistant text, preferring the payload convenience
    field and falling back to the transcript so a Claude Code build that omits
    `last_assistant_message` still delivers instead of silently no-op'ing."""
    candidate = payload.get("last_assistant_message")
    if isinstance(candidate, str) and candidate.strip():
        return candidate
    if agent_kind != "claude":
        return None
    fallback = transcript_final_message(payload.get("transcript_path"))
    if fallback and fallback.strip():
        return fallback
    return None


def stage_response(target: pathlib.Path, body: str) -> pathlib.Path:
    target.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(
        dir=target.parent,
        prefix=f".{target.name}.",
        suffix=".tmp",
    )
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as staged:
            staged.write(body)
            staged.flush()
            os.fsync(staged.fileno())
    except Exception:
        try:
            os.close(descriptor)
        except Exception:
            pass
        pathlib.Path(name).unlink(missing_ok=True)
        raise
    return pathlib.Path(name)


def backup_target(target: pathlib.Path) -> pathlib.Path | None:
    if not target.exists():
        return None
    descriptor, name = tempfile.mkstemp(
        dir=target.parent,
        prefix=f".{target.name}.",
        suffix=".backup",
    )
    os.close(descriptor)
    backup = pathlib.Path(name)
    backup.unlink()
    try:
        os.link(target, backup)
    except Exception:
        backup.unlink(missing_ok=True)
        raise
    return backup


def sync_directory(path: pathlib.Path) -> None:
    descriptor = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def same_file(path: pathlib.Path, identity: os.stat_result) -> bool:
    try:
        current = path.stat()
    except OSError:
        return False
    return (current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino)


def rollback_publication(
    target: pathlib.Path,
    identity: os.stat_result,
    backup: pathlib.Path | None,
) -> None:
    if not same_file(target, identity):
        return
    try:
        if backup is None:
            target.unlink()
        else:
            os.replace(backup, target)
        sync_directory(target.parent)
    except Exception:
        pass


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
    conn = None
    temporary = None
    target = None
    backup = None
    published_identity = None
    published = False
    committed = False
    try:
        payload = json.load(sys.stdin)
        if payload.get("parent_session_id") or payload.get("parentID") or payload.get("parent_id"):
            return
        session_id = payload.get("session_id")
        if not session_id and launch["BRAIN_AGENT_KIND"] == "codex":
            session_id = payload.get("thread_id")
        response_id = os.environ.get("BRAIN_RESPONSE_ID") or session_id
        message = resolve_final_message(payload, launch["BRAIN_AGENT_KIND"])
        if not session_id or not response_id or not message:
            return
        target_dir = pathlib.Path(launch["BRAIN_RESPONSE_DIR"])
        target = target_dir / f"{response_id}.json"
        temporary = stage_response(
            target,
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
        )
        scope = (
            launch["BRAIN_AGENT_KIND"],
            session_id,
            launch["BRAIN_WORKSPACE_ID"],
            launch["BRAIN_ACTOR_ID"],
            launch["BRAIN_CHANNEL"],
            launch["BRAIN_INSTANCE_ID"],
        )
        conn = sqlite3.connect(
            launch["BRAIN_STATE_DB"],
            timeout=5,
            isolation_level=None,
        )
        conn.execute("PRAGMA foreign_keys = ON;")
        conn.execute("PRAGMA busy_timeout = 5000;")
        conn.execute("BEGIN IMMEDIATE")
        registered = conn.execute(
            """
            SELECT 1 FROM brain_sessions
            WHERE agent_kind = ? AND agent_session_id = ? AND workspace_id = ?
              AND actor_id = ? AND channel = ? AND brain_instance_id = ?
              AND locked_pid IS NOT NULL
              AND completion_status = 'active'
            """,
            scope,
        ).fetchone()
        if not registered:
            conn.rollback()
            return
        updated = conn.execute(
            """
            UPDATE brain_sessions SET completion_status = 'completed'
            WHERE agent_kind = ? AND agent_session_id = ? AND workspace_id = ?
              AND actor_id = ? AND channel = ? AND brain_instance_id = ?
              AND locked_pid IS NOT NULL
              AND completion_status = 'active'
            """,
            scope,
        )
        if updated.rowcount != 1:
            raise RuntimeError("completion authorization changed")
        backup = backup_target(target)
        published_identity = temporary.stat()
        os.replace(temporary, target)
        published = True
        sync_directory(target.parent)
        conn.commit()
        committed = True
    except Exception:
        if published and not committed and target is not None and published_identity is not None:
            rollback_publication(target, published_identity, backup)
        if conn is not None:
            try:
                if conn.in_transaction:
                    conn.rollback()
            except Exception:
                pass
    finally:
        if conn is not None:
            try:
                conn.close()
            except Exception:
                pass
        if temporary is not None:
            temporary.unlink(missing_ok=True)
        if backup is not None:
            backup.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
