#!/usr/bin/env python3
"""Local-only rclone boundary for interrupted portable-receive acceptance."""
import json
import os
from pathlib import Path
import shutil
import signal
import sqlite3
import sys

arguments = sys.argv[1:]
remote = Path(os.environ["BRAIN_FIXTURE_REMOTE"])
local = Path(os.environ["BRAIN_FIXTURE_ROOT"])
trace = Path(os.environ["BRAIN_FIXTURE_TRACE"])
mode = os.environ.get("BRAIN_FIXTURE_RECEIVE", "complete")
with trace.open("a") as output:
    output.write(json.dumps(arguments) + "\n")


def path(value):
    prefix = "BRAIN:fixture"
    return remote / value[len(prefix):].lstrip("/") if value.startswith(prefix) else Path(value)


def copy(source, destination):
    if source.is_file():
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)


operation = arguments[0]
if operation == "version":
    print("rclone v1.70.0")
elif operation == "cat":
    source = path(arguments[1])
    if not source.is_file():
        sys.exit(3)
    sys.stdout.buffer.write(source.read_bytes())
elif operation == "lsf":
    assert arguments[2:] == ["--files-only"]
    source = path(arguments[1])
    if source.is_dir():
        print("\n".join(sorted(item.name for item in source.iterdir() if item.is_file())))
elif operation == "copyto":
    source, destination = map(path, arguments[1:3])
    if not source.is_file():
        sys.exit(3)
    copy(source, destination)
elif operation == "copy":
    source, destination = map(path, arguments[1:3])
    if "--files-from" in arguments:
        names = Path(arguments[arguments.index("--files-from") + 1]).read_text().splitlines()
        for name in names:
            copy(source / name, destination / name)
    else:
        assert source == local and destination == remote
        assert (local / "areas").is_dir(), "initial skeleton must precede final push"

elif operation == "bisync":
    assert path(arguments[1]) == local and path(arguments[2]) == remote
    journal = sqlite3.connect(os.environ["BRAIN_FIXTURE_JOURNAL"])
    assert journal.execute("SELECT phase FROM portable_receive WHERE slot=1").fetchone() == ("receiving",)
    journal.close()
    if mode == "initial":
        assert not (local / "areas").exists(), "initial pull must precede PARA seeding"
    copy(remote / "AGENTS.md", local / "AGENTS.md")
    if mode == "crash":
        os.kill(os.getppid(), signal.SIGKILL)
        sys.exit(0)
    if mode == "fail":
        sys.exit(5)
    if mode != "mismatch":
        for source in (remote / ".config/extensions").glob("*.md"):
            copy(source, local / ".config/extensions" / source.name)
    print('{"level":"info","msg":"Transferred: 2 / 2, 100%"}', file=sys.stderr)
else:
    raise AssertionError(f"unexpected receive fixture operation: {operation}")
