#!/usr/bin/env python3
"""Record Claude's final assistant message for an inbound brain job."""
from __future__ import annotations

import json
import os
import pathlib
import sys


def main() -> None:
    directory = os.environ.get("BRAIN_RESPONSE_DIR")
    if not directory:
        return
    try:
        payload = json.load(sys.stdin)
        session_id = payload.get("session_id")
        message = payload.get("last_assistant_message")
        if not session_id or not isinstance(message, str) or not message.strip():
            return
        target_dir = pathlib.Path(directory)
        target_dir.mkdir(parents=True, exist_ok=True)
        target = target_dir / f"{session_id}.json"
        temporary = target.with_suffix(".tmp")
        temporary.write_text(json.dumps({"session_id": session_id, "message": message}), encoding="utf-8")
        temporary.replace(target)
    except Exception:
        return


if __name__ == "__main__":
    main()
