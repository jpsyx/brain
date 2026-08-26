#!/usr/bin/env python3
"""Write content-free receiver lifecycle observations for Brain."""
from __future__ import annotations

import fcntl
import json
import os
import pathlib
import stat
import sys
import time
import unicodedata
import uuid


VERSION = 1
MAX_SNAPSHOT_BYTES = 4096
MAX_REVISION = (1 << 63) - 1
MAX_TIMESTAMP = (1 << 64) - 1
MAX_IDENTIFIER_BYTES = 256
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


def confined_io_available() -> bool:
    return (
        os.name == "posix"
        and hasattr(os, "geteuid")
        and hasattr(os, "fchmod")
        and all(hasattr(os, name) for name in ("O_CLOEXEC", "O_DIRECTORY", "O_NOFOLLOW"))
        and os.open in os.supports_dir_fd
        and os.mkdir in os.supports_dir_fd
        and os.rename in os.supports_dir_fd
        and os.stat in os.supports_dir_fd
        and os.unlink in os.supports_dir_fd
    )


def valid_directory(value: os.stat_result, owner_only: bool) -> bool:
    return (
        stat.S_ISDIR(value.st_mode)
        and (not owner_only or value.st_uid == os.geteuid())
        and (not owner_only or stat.S_IMODE(value.st_mode) & 0o077 == 0)
    )


def open_confined_parent(path: pathlib.Path) -> int | None:
    if not confined_io_available() or not path.is_absolute() or path.name in ("", ".", ".."):
        return None
    parts = path.parent.parts
    if not parts or parts[0] != "/" or any(part in ("", ".", "..") for part in parts[1:]):
        return None
    flags = os.O_RDONLY | os.O_CLOEXEC | os.O_DIRECTORY | os.O_NOFOLLOW
    try:
        descriptor = os.open("/", flags)
    except OSError:
        return None
    keep_open = False
    try:
        for index, part in enumerate(parts[1:], start=1):
            owner_only = index >= len(parts) - 2
            try:
                child = os.open(part, flags, dir_fd=descriptor)
            except FileNotFoundError:
                if not valid_directory(os.fstat(descriptor), True):
                    return None
                os.mkdir(part, 0o700, dir_fd=descriptor)
                child = os.open(part, flags, dir_fd=descriptor)
            facts = os.fstat(child)
            if (
                owner_only
                and stat.S_ISDIR(facts.st_mode)
                and facts.st_uid == os.geteuid()
                and stat.S_IMODE(facts.st_mode) & 0o077 != 0
            ):
                os.fchmod(child, 0o700)
                facts = os.fstat(child)
            if not valid_directory(facts, owner_only):
                os.close(child)
                return None
            os.close(descriptor)
            descriptor = child
        keep_open = True
        return descriptor
    except OSError:
        return None
    finally:
        if not keep_open:
            os.close(descriptor)


def same_entry(directory: int, name: str, identity: os.stat_result) -> bool:
    try:
        current = os.stat(name, dir_fd=directory, follow_symlinks=False)
    except OSError:
        return False
    return opened_file_facts(current) == opened_file_facts(identity)


def owner_only_lock(directory: int, name: str) -> int | None:
    flags = os.O_RDWR | os.O_CLOEXEC | os.O_NOFOLLOW
    try:
        try:
            descriptor = os.open(name, flags | os.O_CREAT | os.O_EXCL, 0o600, dir_fd=directory)
        except FileExistsError:
            descriptor = os.open(name, flags, dir_fd=directory)
        facts = os.fstat(descriptor)
        if not valid_opened_file(facts) or not same_entry(directory, name, facts):
            os.close(descriptor)
            return None
        return descriptor
    except OSError:
        return None


def write_snapshot(directory: int, name: str, snapshot: dict[str, object]) -> bool:
    body = json.dumps(snapshot, separators=(",", ":"), sort_keys=True)
    encoded = body.encode("utf-8")
    if len(encoded) > MAX_SNAPSHOT_BYTES:
        return False
    temporary_name = None
    descriptor = None
    temporary_identity = None
    try:
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW
        for _ in range(32):
            candidate = f".{name}.{uuid.uuid4().hex}.tmp"
            try:
                descriptor = os.open(candidate, flags, 0o600, dir_fd=directory)
                temporary_name = candidate
                break
            except FileExistsError:
                continue
        if descriptor is None or temporary_name is None:
            return False
        with os.fdopen(descriptor, "wb") as output:
            output.write(encoded)
            output.flush()
            os.fsync(output.fileno())
        descriptor = None
        temporary_identity = os.stat(
            temporary_name,
            dir_fd=directory,
            follow_symlinks=False,
        )
        if not valid_opened_file(temporary_identity):
            return False
        os.replace(
            temporary_name,
            name,
            src_dir_fd=directory,
            dst_dir_fd=directory,
        )
        if not same_entry(directory, name, temporary_identity):
            return False
        os.fsync(directory)
        return True
    except BaseException:
        if descriptor is not None:
            try:
                os.close(descriptor)
            except OSError:
                pass
        raise
    finally:
        if (
            temporary_name is not None
            and temporary_identity is not None
            and same_entry(directory, temporary_name, temporary_identity)
        ):
            try:
                os.unlink(temporary_name, dir_fd=directory)
            except OSError:
                pass


def valid_identifier(value: object) -> bool:
    return (
        isinstance(value, str)
        and bool(value)
        and len(value.encode("utf-8")) <= MAX_IDENTIFIER_BYTES
        and not any(unicodedata.category(character) == "Cc" for character in value)
    )


def valid_timestamp(value: object) -> bool:
    return type(value) is int and 0 <= value <= MAX_TIMESTAMP


def unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    value: dict[str, object] = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate JSON field")
        value[key] = item
    return value


def opened_file_facts(value: os.stat_result) -> tuple[int, int, int, int, int]:
    return (
        value.st_dev,
        value.st_ino,
        stat.S_IFMT(value.st_mode),
        stat.S_IMODE(value.st_mode),
        value.st_size,
    )


def valid_opened_file(value: os.stat_result) -> bool:
    return (
        stat.S_ISREG(value.st_mode)
        and value.st_uid == os.geteuid()
        and stat.S_IMODE(value.st_mode) & 0o077 == 0
        and 0 <= value.st_size <= MAX_SNAPSHOT_BYTES
    )


def valid_snapshot(value: object) -> bool:
    if not isinstance(value, dict) or set(value) != FIELDS:
        return False
    if type(value.get("version")) is not int or value["version"] != VERSION:
        return False
    revision = value.get("revision")
    if type(revision) is not int or not 1 <= revision <= MAX_REVISION:
        return False
    if canonical_token(value.get("job_token")) is None:
        return False
    if canonical_token(value.get("instance_id")) is None:
        return False
    if not valid_identifier(value.get("session_id")):
        return False
    turn_id = value.get("turn_id")
    if turn_id is not None and not valid_identifier(turn_id):
        return False
    timestamps = [
        value.get("accepted_at_unix_ms"),
        value.get("progressing_at_unix_ms"),
        value.get("completed_at_unix_ms"),
    ]
    if any(timestamp is not None and not valid_timestamp(timestamp) for timestamp in timestamps):
        return False
    accepted, progressing, completed = timestamps
    phase = value.get("phase")
    consistent = (
        phase == "accepted"
        and accepted is not None
        and progressing is None
        and completed is None
    ) or (
        phase == "progressing"
        and accepted is not None
        and progressing is not None
        and completed is None
    ) or (
        phase == "completed"
        and completed is not None
        and not (accepted is None and progressing is not None)
    )
    present = [timestamp for timestamp in timestamps if timestamp is not None]
    return consistent and present == sorted(present)


def read_snapshot(directory: int, name: str) -> tuple[bool, dict[str, object] | None]:
    flags = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW
    for flag in ("O_NONBLOCK", "O_NOCTTY"):
        flags |= getattr(os, flag, 0)
    try:
        descriptor = os.open(name, flags, dir_fd=directory)
    except FileNotFoundError:
        return True, None
    except OSError:
        return False, None
    try:
        before = os.fstat(descriptor)
        if not valid_opened_file(before):
            return False, None
        try:
            entry_before = os.stat(name, dir_fd=directory, follow_symlinks=False)
        except OSError:
            return False, None
        if opened_file_facts(entry_before) != opened_file_facts(before):
            return False, None
        encoded = os.read(descriptor, MAX_SNAPSHOT_BYTES + 1)
        after = os.fstat(descriptor)
        try:
            entry_after = os.stat(name, dir_fd=directory, follow_symlinks=False)
        except OSError:
            return False, None
        if (
            not valid_opened_file(after)
            or opened_file_facts(before) != opened_file_facts(after)
            or opened_file_facts(after) != opened_file_facts(entry_after)
            or len(encoded) != before.st_size
        ):
            return False, None
    except OSError:
        return False, None
    finally:
        os.close(descriptor)
    try:
        value = json.loads(encoded, object_pairs_hook=unique_object)
    except (UnicodeDecodeError, ValueError):
        return False, None
    return (True, value) if valid_snapshot(value) else (False, None)


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
            updated = accepted_snapshot(token, instance, session, now)
            return updated if valid_snapshot(updated) else None
        return None
    if phase == "completed" and current is None:
        updated = {
            "version": VERSION,
            "revision": 1,
            "phase": "completed",
            "job_token": token,
            "instance_id": instance,
            "session_id": session,
            "turn_id": turn_id if valid_identifier(turn_id) else None,
            "accepted_at_unix_ms": None,
            "progressing_at_unix_ms": None,
            "completed_at_unix_ms": now,
        }
        return updated if valid_snapshot(updated) else None
    if current is None or not same_scope(current, token, instance, session):
        return None
    if current["revision"] >= MAX_REVISION:
        return None
    prior_timestamps = [
        timestamp
        for timestamp in (
            current.get("accepted_at_unix_ms"),
            current.get("progressing_at_unix_ms"),
            current.get("completed_at_unix_ms"),
        )
        if valid_timestamp(timestamp)
    ]
    monotonic_now = max([now, *prior_timestamps])
    if phase == "progressing" and current.get("phase") == "accepted":
        updated = dict(current)
        updated.update(
            revision=current["revision"] + 1,
            phase="progressing",
            turn_id=turn_id if valid_identifier(turn_id) else None,
            progressing_at_unix_ms=monotonic_now,
        )
        return updated if valid_snapshot(updated) else None
    if phase == "completed" and current.get("phase") in ("accepted", "progressing"):
        updated = dict(current)
        updated.update(
            revision=current["revision"] + 1,
            phase="completed",
            turn_id=(
                turn_id
                if valid_identifier(turn_id)
                else current.get("turn_id")
            ),
            completed_at_unix_ms=monotonic_now,
        )
        return updated if valid_snapshot(updated) else None
    return None


def main() -> bool:
    token = canonical_token(os.environ.get("BRAIN_RECEIVER_JOB_TOKEN"))
    observation_path = os.environ.get("BRAIN_RECEIVER_OBSERVATION_PATH")
    instance_id = canonical_token(os.environ.get("BRAIN_INSTANCE_ID"))
    if not token or not observation_path or not instance_id:
        return False
    try:
        payload = json.load(sys.stdin)
    except Exception:
        return False
    if not isinstance(payload, dict):
        return False
    if (
        payload.get("agent_id")
        or payload.get("parent_session_id")
        or payload.get("parentID")
        or payload.get("parent_id")
    ):
        return False
    session_id = payload.get("session_id")
    if not session_id and os.environ.get("BRAIN_AGENT_KIND") == "codex":
        session_id = payload.get("thread_id")
    if not valid_identifier(session_id):
        return False
    event = payload.get("hook_event_name")
    if event == "UserPromptSubmit":
        phase = "accepted"
        if not terminal_marker_matches(payload.get("prompt"), token):
            return False
    elif event == "PostToolUse":
        phase = "progressing"
    elif event == "Stop":
        phase = "completed"
    else:
        return False

    target = pathlib.Path(observation_path)
    directory = open_confined_parent(target)
    if directory is None:
        return False
    descriptor = owner_only_lock(directory, f"{target.name}.lock")
    if descriptor is None:
        os.close(directory)
        return False
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX)
        now = time.time_ns() // 1_000_000
        prior_is_trusted, current = read_snapshot(directory, target.name)
        if not prior_is_trusted:
            return False
        updated = next_snapshot(
            current,
            phase,
            token,
            instance_id,
            session_id,
            payload.get("turn_id"),
            now,
        )
        if updated is not None and valid_snapshot(updated):
            return write_snapshot(directory, target.name, updated)
        return bool(
            phase == "completed"
            and current is not None
            and current.get("phase") == "completed"
            and same_scope(current, token, instance_id, session_id)
        )
    finally:
        os.close(descriptor)
        os.close(directory)


if __name__ == "__main__":
    os.umask(0o077)
    try:
        succeeded = main()
    except Exception:
        succeeded = False
    if "--require-write" in sys.argv[1:] and not succeeded:
        raise SystemExit(1)
