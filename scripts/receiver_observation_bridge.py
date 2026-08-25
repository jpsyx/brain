#!/usr/bin/env python3
"""Write content-free receiver lifecycle observations for Brain."""
from __future__ import annotations

import fcntl
import json
import os
import pathlib
import sys
import tempfile
import time
import uuid


VERSION = 1
MAX_SNAPSHOT_BYTES = 4096
FIELDS = {
    "version",
    "revision",
    "phase",
    "job_token",
    "instance_id",
    "session_id",
    "turn_id",
    "accepted_at_unix_ms",
    "progressing_at_unix_ms",
    "completed_at_unix_ms",
}


def canonical_token(value: object) -> str | None:
    if not isinstance(value, str):
        return None
    try:
        parsed = uuid.UUID(value)
    except (ValueError, AttributeError):
        return None
    rendered = str(parsed)
    return rendered if rendered == value else None


def terminal_marker_matches(prompt: object, token: str) -> bool:
    if not isinstance(prompt, str):
        return False
    lines = prompt.splitlines()
    return bool(lines) and lines[-1] == f"<!-- brain:receiver-job-token={token} -->"


def owner_only_file(path: pathlib.Path) -> int:
    descriptor = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
    os.fchmod(descriptor, 0o600)
    return descriptor


def sync_directory(path: pathlib.Path) -> None:
    descriptor = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def write_snapshot(path: pathlib.Path, snapshot: dict[str, object]) -> None:
    body = json.dumps(snapshot, separators=(",", ":"), sort_keys=True)
    encoded = body.encode("utf-8")
    if len(encoded) > MAX_SNAPSHOT_BYTES:
        return
    descriptor, temporary_name = tempfile.mkstemp(
        dir=path.parent,
        prefix=f".{path.name}.",
        suffix=".tmp",
    )
    temporary = pathlib.Path(temporary_name)
    try:
        os.fchmod(descriptor, 0o600)
        with os.fdopen(descriptor, "wb") as output:
            output.write(encoded)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        os.chmod(path, 0o600)
        sync_directory(path.parent)
    except BaseException:
        try:
            os.close(descriptor)
        except OSError:
            pass
        temporary.unlink(missing_ok=True)
        raise


def read_snapshot(path: pathlib.Path) -> dict[str, object] | None:
    try:
        with open(path, "rb") as source:
            encoded = source.read(MAX_SNAPSHOT_BYTES + 1)
    except OSError:
        return None
    if len(encoded) > MAX_SNAPSHOT_BYTES:
        return None
    try:
        value = json.loads(encoded)
    except (UnicodeDecodeError, ValueError):
        return None
    if not isinstance(value, dict) or set(value) != FIELDS:
        return None
    if value.get("version") != VERSION:
        return None
    if not isinstance(value.get("revision"), int) or value["revision"] < 1:
        return None
    if value.get("phase") not in ("accepted", "progressing", "completed"):
        return None
    return value


def same_scope(snapshot: dict[str, object], token: str, instance: str, session: str) -> bool:
    return (
        snapshot.get("job_token") == token
        and snapshot.get("instance_id") == instance
        and snapshot.get("session_id") == session
    )


def accepted_snapshot(token: str, instance: str, session: str, now: int) -> dict[str, object]:
    return {
        "version": VERSION,
        "revision": 1,
        "phase": "accepted",
        "job_token": token,
        "instance_id": instance,
        "session_id": session,
        "turn_id": None,
        "accepted_at_unix_ms": now,
        "progressing_at_unix_ms": None,
        "completed_at_unix_ms": None,
    }


def next_snapshot(
    current: dict[str, object] | None,
    phase: str,
    token: str,
    instance: str,
    session: str,
    turn_id: object,
    now: int,
) -> dict[str, object] | None:
    if phase == "accepted":
        if current is None:
            return accepted_snapshot(token, instance, session, now)
        return None
    if phase == "completed" and current is None:
        return {
            "version": VERSION,
            "revision": 1,
            "phase": "completed",
            "job_token": token,
            "instance_id": instance,
            "session_id": session,
            "turn_id": turn_id if isinstance(turn_id, str) and turn_id else None,
            "accepted_at_unix_ms": None,
            "progressing_at_unix_ms": None,
            "completed_at_unix_ms": now,
        }
    if current is None or not same_scope(current, token, instance, session):
        return None
    if phase == "progressing" and current.get("phase") == "accepted":
        updated = dict(current)
        updated.update(
            revision=current["revision"] + 1,
            phase="progressing",
            turn_id=turn_id if isinstance(turn_id, str) and turn_id else None,
            progressing_at_unix_ms=now,
        )
        return updated
    if phase == "completed" and current.get("phase") in ("accepted", "progressing"):
        updated = dict(current)
        updated.update(
            revision=current["revision"] + 1,
            phase="completed",
            turn_id=(
                turn_id
                if isinstance(turn_id, str) and turn_id
                else current.get("turn_id")
            ),
            completed_at_unix_ms=now,
        )
        return updated
    return None


def main() -> None:
    token = canonical_token(os.environ.get("BRAIN_RECEIVER_JOB_TOKEN"))
    observation_path = os.environ.get("BRAIN_RECEIVER_OBSERVATION_PATH")
    instance_id = os.environ.get("BRAIN_INSTANCE_ID")
    if not token or not observation_path or not instance_id:
        return
    try:
        payload = json.load(sys.stdin)
    except Exception:
        return
    if not isinstance(payload, dict):
        return
    if (
        payload.get("agent_id")
        or payload.get("parent_session_id")
        or payload.get("parentID")
        or payload.get("parent_id")
    ):
        return
    session_id = payload.get("session_id")
    if not session_id and os.environ.get("BRAIN_AGENT_KIND") == "codex":
        session_id = payload.get("thread_id")
    if not isinstance(session_id, str) or not session_id:
        return
    event = payload.get("hook_event_name")
    if event == "UserPromptSubmit":
        phase = "accepted"
        if not terminal_marker_matches(payload.get("prompt"), token):
            return
    elif event == "PostToolUse":
        phase = "progressing"
    elif event == "Stop":
        phase = "completed"
    else:
        return

    target = pathlib.Path(observation_path)
    target.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    lock_path = pathlib.Path(f"{target}.lock")
    descriptor = owner_only_file(lock_path)
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX)
        now = time.time_ns() // 1_000_000
        current = read_snapshot(target)
        updated = next_snapshot(
            current,
            phase,
            token,
            instance_id,
            session_id,
            payload.get("turn_id"),
            now,
        )
        if updated is not None:
            write_snapshot(target, updated)
    finally:
        os.close(descriptor)


if __name__ == "__main__":
    os.umask(0o077)
    try:
        main()
    except Exception:
        pass
