#!/usr/bin/env python3
"""Compatibility launcher for Brain's generic session-start bridge."""

from pathlib import Path
import runpy


runpy.run_path(
    str(Path(__file__).with_name("agent_session_start_hook.py")),
    run_name="__main__",
)
