#!/usr/bin/env python3
"""Update the day's agenda after a tasks/habits.csv mutation.

Idempotent — safe to call after every CSV mutation. Exits cleanly (rc=0)
whatever happens: the CSV is the source of truth and the agenda is
downstream, so a failed agenda update must never fail the mutation that
already succeeded.

**This script no longer implements the sync.** The section-preserving logic
lives in the brain binary (`brain tasks sync-agenda`), which brain's own
native completion (`brain tasks complete`, and the tasks view's
mark-complete) also runs in-process. Having exactly one implementation is the
point: the two used to be able to drift, and a native completion silently
left the agenda stale.

Actions map onto the binary's three:

- `done`     — the row was completed: drop it from the MIT callout /
               Suggested order / Cut order, handing a chunked task's slot to
               its next unfinished chunk.
- `defer`    — the row moved off today: drop it from those sections.
- `backlog`  — the row left today's plan entirely; same as `defer`.
- `touch`    — the row is still on today's plan: refresh only the
               CSV-derived snapshots (Today's habits, Completed today).
- `restore`  — the row came back; the plan can't be rebuilt from here, so
               this refreshes the snapshots like `touch`.

Today's habits and Completed today are re-derived from the CSVs on every
run, which catches habits flipped to done outside this session (other agent
runs, /triage, manual edits). The printable is regenerated only if one
already exists.

Usage:
    update_agenda_on_mutation.py <task_id> done|defer|backlog|touch|restore
"""
import argparse
import os
import re
import shutil
import subprocess
import sys

# How a caller's action maps onto `brain tasks sync-agenda --action`.
ACTIONS = {
    "done": "done",
    "defer": "defer",
    "backlog": "defer",
    "touch": "touch",
    "restore": "touch",
}

# A slow agenda update must never wedge a mutator script.
TIMEOUT_SECONDS = 60


def log(msg: str) -> None:
    print(f"[update_agenda] {msg}", file=sys.stderr)


def _render_ready_markdown(text: str) -> str:
    """Strip HTML comments before handing markdown to `markdown-to-pdf`.

    `markdown-to-pdf` is a bespoke line-based renderer with no concept of
    HTML — it has no comment-stripping logic at all, so a raw HTML comment
    (e.g. bake_triage_appendix.py's idempotency marker,
    `<!-- brain:optional-content -->`) shows up as literal visible text on
    the PDF page instead of disappearing the way it would in a real
    markdown-to-HTML renderer. The marker must stay in the *source* file —
    bake_triage_appendix.py greps for it to find and replace the appendix
    section idempotently on every later run — so only the copy handed to
    the PDF renderer gets comments stripped, never the agenda itself.

    Kept here for the agenda-*build* flow, which renders a PDF the mutation
    path never touches. The mutation path's own regen is inside the binary.
    """
    return re.sub(r"[ \t]*<!--.*?-->", "", text, flags=re.DOTALL)


def brain_argv(task_id: str, action: str) -> list[str] | None:
    """The `brain tasks sync-agenda` invocation, or None with no binary.

    The workspace is named explicitly: these scripts are launched through
    brain, so `BRAIN_WORKSPACE` names the workspace whose CSVs were mutated,
    and a sync must never land on a different one.
    """
    binary = (os.environ.get("BRAIN_BIN") or "").strip() or shutil.which("brain")
    if not binary:
        return None
    argv = [binary]
    workspace = (os.environ.get("BRAIN_WORKSPACE") or "").strip()
    if workspace:
        argv += ["-b", workspace]
    return argv + ["tasks", "sync-agenda", task_id, "--action", action]


def main() -> int:
    parser = argparse.ArgumentParser(description="Update the day's agenda after a CSV mutation.")
    parser.add_argument("task_id", help="T### or H### task id")
    parser.add_argument("action", choices=sorted(ACTIONS))
    args = parser.parse_args()

    argv = brain_argv(args.task_id.strip().upper(), ACTIONS[args.action])
    if argv is None:
        log("no brain binary on PATH (set BRAIN_BIN); skipping the agenda update")
        return 0
    try:
        result = subprocess.run(
            argv,
            check=False,
            capture_output=True,
            text=True,
            timeout=TIMEOUT_SECONDS,
        )
    except (OSError, subprocess.SubprocessError) as e:
        log(f"could not update the agenda: {e}")
        return 0
    if result.returncode != 0:
        log(f"agenda update failed (rc={result.returncode}): {(result.stderr or '').strip()}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
