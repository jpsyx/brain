#!/usr/bin/env python3
"""Update today's agenda in-place after a tasks/habits.csv mutation.

Idempotent — safe to call after every CSV mutation. Exits cleanly (rc=0)
when there's nothing to do (no agenda file for today, task not on
agenda, etc.).

Per /todo SKILL.md operating principle 7 (the "task-mutation auto-update
checklist"), every mutator that touches tasks.csv or habits.csv must
update `/tmp/<today>.md` and regen `$AGENDA_DIR/agenda-<today>.pdf` when
appropriate. This script is the implementation — callers (`defer_task.py`,
`defer_task.py`, `defer_habit.py`, `touch_task.py`) invoke it at the
end of a successful mutation so the LLM doesn't have to.

Actions:
- `done`  — drop the task from MIT callout / Suggested order / Cut order,
            and add it to "Completed today" (which is rebuilt from the
            CSVs). If the completed task is a chunk and the next chunk in
            the family isn't already on the agenda, the next chunk swaps
            into the just-vacated MIT-callout + Suggested-order slot so
            the user always has exactly one actionable chunk visible.
- `defer` — drop the task from MIT callout / Suggested order / Cut order.
- `touch` — no body mutations; Today's habits and Completed today are
            still re-derived from the CSVs so the snapshot stays honest.

Today's habits and Completed today are ALWAYS re-derived from the CSVs
on every run — per the SKILL.md "mandatory re-derive procedure". This
catches habits flipped to done outside our session (other Claude runs,
/triage, manual edits).

The PDF at `$AGENDA_DIR/agenda-<today>.pdf` is regenerated only if it
already exists (the operating-principle-7 carve-out: a CSV mutation
isn't a request for a fresh printout — but if a PDF is on disk it must
stay current).

Failures are logged to stderr but never propagated. The CSV is the
source of truth; the agenda is downstream and best-effort.

Usage:
    update_agenda_on_mutation.py <task_id> done|defer|touch
"""
import argparse
import os
import shutil
import re
import subprocess
import sys
from pathlib import Path

from _csvlib import (
    HABITS_CSV, TASKS_CSV, find_next_chunk, parse_chunk_name, read_csv, today_iso,
)


AGENDA_MD = Path("/tmp") / f"{today_iso()}.md"
# Where the agenda PDF lives: BRAIN_AGENDA_DIR if set (brain passes the
# configured `agenda_dir`), else the user's default download folder.
_AGENDA_DIR = Path(
    os.environ.get("BRAIN_AGENDA_DIR") or (Path.home() / "Downloads")
).expanduser()
AGENDA_PDF = _AGENDA_DIR / f"agenda-{today_iso()}.pdf"
# The PDF renderer: a `markdown-to-pdf` command resolved from PATH (or the
# MARKDOWN_TO_PDF env override brain can pass from `markdown_to_pdf_path`).
MD2PDF = os.environ.get("MARKDOWN_TO_PDF") or "markdown-to-pdf"

# Section heading prefixes. We match by prefix rather than exact heading
# text so minor LLM phrasing variations don't break the script.
H_MIT = "## ❗"
H_SUGGESTED = "## Suggested order"
H_CUT = "## Cut order"
H_HABITS = "## 🔁"
H_COMPLETED = "## ✅"
# The triage appendix baked in by bake_triage_appendix.py. These sections must
# stay at the very bottom of the agenda; a re-derived "Today's habits" /
# "Completed today" that has to be *appended* goes BEFORE them, never after.
H_APPENDIX_PREFIXES = ("## 📧", "## 📰")

# Suggested-order line: "<n>. [ ] <time> | <body>". We preserve the
# numbered prefix and time slot when swapping a chunked task's body.
SUGGESTED_LINE_RE = re.compile(r"^(\d+)\.\s+\[ \]\s+(.+?)\s+\|\s+(.+)$")
# Any "<n>. <rest>" top-level numbered line. Used for renumbering after
# a removal in Suggested order / Cut order.
NUMBERED_LINE_RE = re.compile(r"^(\d+)\.\s+(.*)$")


def log(msg: str) -> None:
    print(f"[update_agenda] {msg}", file=sys.stderr)


# === markdown splitting / joining ===

def _split_sections(text: str):
    """Split markdown into (preamble_lines, [(heading_line, body_lines), ...]).

    A section starts at a "## " heading and runs until the next "## " or
    EOF. Preamble is everything before the first "## " (title, Load,
    Bottom line). Section headings deeper than "## " (e.g. "### foo")
    are treated as body content.
    """
    lines = text.splitlines()
    preamble = []
    sections = []
    i = 0
    while i < len(lines) and not lines[i].startswith("## "):
        preamble.append(lines[i])
        i += 1
    while i < len(lines):
        head = lines[i]
        i += 1
        body = []
        while i < len(lines) and not lines[i].startswith("## "):
            body.append(lines[i])
            i += 1
        sections.append([head, body])
    return preamble, sections


def _join_doc(preamble, sections, trailing_newline: bool) -> str:
    out = list(preamble)
    for head, body in sections:
        out.append(head)
        out.extend(body)
    text = "\n".join(out)
    if trailing_newline:
        text += "\n"
    return text


def _find_section(sections, prefix: str):
    for i, sec in enumerate(sections):
        if sec[0].startswith(prefix):
            return i
    return None


# === line-level mutations ===

def _line_has_id(line: str, task_id: str) -> bool:
    return f"**{task_id}**" in line


def _renumber_numbered(body):
    """Rewrite numbered-list prefixes to a fresh 1..N sequence. Lines that
    don't start with "<n>. " (blank lines, sub-bullets, prose) pass through
    unchanged."""
    counter = 0
    out = []
    for ln in body:
        m = NUMBERED_LINE_RE.match(ln)
        if m:
            counter += 1
            ln = f"{counter}. {m.group(2)}"
        out.append(ln)
    return out


def _drop_lines_with_id(body, task_id: str, renumber: bool):
    kept = [ln for ln in body if not _line_has_id(ln, task_id)]
    return _renumber_numbered(kept) if renumber else kept


def _format_duration_suffix(estimated_duration: str) -> str:
    d = (estimated_duration or "").strip()
    return f" ({d}m)" if d.isdigit() else ""


def _swap_chunk_in_suggested(body, completed_id: str, next_row: dict):
    """If `next_row` (the next chunk in the family) isn't already in the
    Suggested order, replace the completed chunk's line in place — keep
    the "<n>. [ ] <time> | " prefix, swap the body to the next chunk's
    ID + name + duration. Else fall back to plain drop+renumber."""
    next_id = (next_row.get("task_id") or "").strip()
    next_name = (next_row.get("task_name") or "").strip()
    next_dur = (next_row.get("estimated_duration") or "").strip()

    if any(_line_has_id(ln, next_id) for ln in body):
        return _drop_lines_with_id(body, completed_id, renumber=True)

    out = []
    swapped = False
    for ln in body:
        if not swapped and _line_has_id(ln, completed_id):
            m = SUGGESTED_LINE_RE.match(ln)
            if m:
                dur_suffix = _format_duration_suffix(next_dur)
                ln = f"{m.group(1)}. [ ] {m.group(2)} | **{next_id}** {next_name}{dur_suffix}"
                swapped = True
            else:
                continue
        out.append(ln)
    if not swapped:
        return _drop_lines_with_id(body, completed_id, renumber=True)
    return _renumber_numbered(out)


def _swap_chunk_in_mit(body, completed_id: str, next_row: dict):
    """Replace the completed chunk's MIT-callout line with the next chunk's
    (the completion path has already migrated the `mit` tag in the CSV). If the
    next chunk already has a MIT line, just drop the completed one."""
    next_id = (next_row.get("task_id") or "").strip()
    next_name = (next_row.get("task_name") or "").strip()
    next_dur = (next_row.get("estimated_duration") or "").strip()

    if any(_line_has_id(ln, next_id) for ln in body):
        return _drop_lines_with_id(body, completed_id, renumber=False)

    out = []
    swapped = False
    for ln in body:
        if not swapped and _line_has_id(ln, completed_id):
            dur_suffix = _format_duration_suffix(next_dur)
            ln = f"- [ ] ❗ **{next_id}** {next_name}{dur_suffix}"
            swapped = True
        out.append(ln)
    return out


def _maybe_next_chunk_row(task_id: str):
    """If `task_id` is a tasks.csv chunk with an unfinished next sibling,
    return that sibling's row. Else None. Only called after the CSV
    mutation, so the just-completed row is already status=done in disk."""
    if not task_id.startswith("T"):
        return None
    _, rows = read_csv(TASKS_CSV)
    completed = next(
        (r for r in rows if (r.get("task_id") or "").strip() == task_id),
        None,
    )
    if completed is None:
        return None
    if not parse_chunk_name(completed.get("task_name") or ""):
        return None
    _, nxt = find_next_chunk(rows, completed)
    if nxt is None:
        return None
    if (nxt.get("status") or "").strip() == "done":
        return None
    return nxt


# === Today's habits / Completed today re-derivation ===

def _parse_int(s, default):
    s = (s or "").strip()
    try:
        return int(s)
    except ValueError:
        return default


def _habit_sort_key(h):
    """Sort: empty ideal_time last; otherwise ideal_time asc, then
    estimated_duration asc, then task_name."""
    ideal = (h.get("ideal_time") or "").strip()
    ideal_key = (1, "") if not ideal else (0, ideal)
    dur = _parse_int(h.get("estimated_duration"), default=10 ** 9)
    name = (h.get("task_name") or "").lower()
    return (ideal_key, dur, name)


def _render_today_habits_section():
    """Return [heading, body_lines] for "🔁 Today's habits", or None if
    zero habits qualify (caller should omit the section)."""
    _, habits = read_csv(HABITS_CSV)
    t = today_iso()
    pending, done_today = [], []
    for h in habits:
        status = (h.get("status") or "").strip()
        due = (h.get("due_date") or "").strip()
        comp = (h.get("completed_date") or "").strip()
        if status == "done" and comp == t:
            done_today.append(h)
        elif status != "done" and (not due or due <= t):
            pending.append(h)
    pending.sort(key=_habit_sort_key)
    done_today.sort(key=_habit_sort_key)

    cells = [f"◻ **{h['task_id']}** {h['task_name']}" for h in pending]
    cells += [f"✅ **{h['task_id']}** {h['task_name']}" for h in done_today]
    if not cells:
        return None
    while len(cells) % 2 != 0:
        cells.append("")

    body = ["", "|  |  |", "|---|---|"]
    for i in range(0, len(cells), 2):
        body.append(f"| {cells[i]} | {cells[i+1]} |")
    body.append("")
    return ["## 🔁 Today's habits", body]


def _render_completed_today_section():
    """Return [heading, body_lines] for "✅ Completed today", or None if
    nothing's completed today."""
    t = today_iso()
    done_rows = []
    for path in (HABITS_CSV, TASKS_CSV):
        _, rows = read_csv(path)
        for r in rows:
            if (r.get("status") or "").strip() == "done" and (r.get("completed_date") or "").strip() == t:
                done_rows.append(r)
    cells = [f"✅ **{r['task_id']}** {r['task_name']}" for r in done_rows]
    if not cells:
        return None
    while len(cells) % 2 != 0:
        cells.append("")

    body = ["", "|  |  |", "|---|---|"]
    for i in range(0, len(cells), 2):
        body.append(f"| {cells[i]} | {cells[i+1]} |")
    body.append("")
    return ["## ✅ Completed today", body]


def _first_appendix_index(sections):
    """Index of the first triage-appendix section (## 📧 / ## 📰), or None."""
    for i, sec in enumerate(sections):
        if sec[0].startswith(H_APPENDIX_PREFIXES):
            return i
    return None


def _replace_or_set_section(sections, prefix: str, new_section):
    """Replace the section matched by `prefix` with `new_section`. If no
    section matches and `new_section` isn't None, insert it — before the triage
    appendix if one is present, else append at the end. If a section matches and
    `new_section` is None, remove it.
    """
    idx = _find_section(sections, prefix)
    if idx is None:
        if new_section is not None:
            appendix_idx = _first_appendix_index(sections)
            if appendix_idx is None:
                sections.append(new_section)
            else:
                sections.insert(appendix_idx, new_section)
        return
    if new_section is None:
        sections.pop(idx)
    else:
        sections[idx] = new_section


def _regen_pdf():
    """Regen $AGENDA_DIR/agenda-<today>.pdf if it already exists. Honors
    the SKILL.md operating-principle-7 carve-out: no PDF on disk → skip."""
    if not AGENDA_PDF.exists():
        return
    if shutil.which(MD2PDF) is None and not Path(MD2PDF).exists():
        log(f"markdown-to-pdf command not found ({MD2PDF}); skipping PDF regen")
        return
    # Any appended sections baked into AGENDA_MD (e.g. a personal triage
    # appendix) are re-rendered automatically by rebuilding from the markdown.
    try:
        AGENDA_PDF.unlink()
        subprocess.run(
            [MD2PDF, str(AGENDA_MD), "--out", str(AGENDA_PDF), "--agenda"],
            check=True,
            capture_output=True,
            text=True,
        )
    except subprocess.CalledProcessError as e:
        log(f"PDF regen failed (rc={e.returncode}): {e.stderr.strip() or e.stdout.strip()}")
    except OSError as e:
        log(f"PDF regen failed: {e}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Update today's agenda after a CSV mutation.")
    parser.add_argument("task_id", help="T### or H### task id")
    parser.add_argument("action", choices=["done", "defer", "touch"])
    args = parser.parse_args()
    task_id = args.task_id.strip().upper()

    if not AGENDA_MD.exists():
        return 0

    try:
        text = AGENDA_MD.read_text()
    except OSError as e:
        log(f"could not read agenda: {e}")
        return 0
    trailing_newline = text.endswith("\n")
    preamble, sections = _split_sections(text)

    if args.action in ("done", "defer"):
        next_chunk = _maybe_next_chunk_row(task_id) if args.action == "done" else None

        mit_idx = _find_section(sections, H_MIT)
        if mit_idx is not None:
            head, body = sections[mit_idx]
            if next_chunk is not None:
                body = _swap_chunk_in_mit(body, task_id, next_chunk)
            else:
                body = _drop_lines_with_id(body, task_id, renumber=False)
            sections[mit_idx] = [head, body]

        sug_idx = _find_section(sections, H_SUGGESTED)
        if sug_idx is not None:
            head, body = sections[sug_idx]
            if next_chunk is not None:
                body = _swap_chunk_in_suggested(body, task_id, next_chunk)
            else:
                body = _drop_lines_with_id(body, task_id, renumber=True)
            sections[sug_idx] = [head, body]

        cut_idx = _find_section(sections, H_CUT)
        if cut_idx is not None:
            head, body = sections[cut_idx]
            body = _drop_lines_with_id(body, task_id, renumber=True)
            sections[cut_idx] = [head, body]

    new_habits = _render_today_habits_section()
    _replace_or_set_section(sections, H_HABITS, new_habits)

    new_completed = _render_completed_today_section()
    _replace_or_set_section(sections, H_COMPLETED, new_completed)

    out = _join_doc(preamble, sections, trailing_newline)
    try:
        AGENDA_MD.write_text(out)
    except OSError as e:
        log(f"could not write agenda: {e}")
        return 0

    _regen_pdf()
    return 0


if __name__ == "__main__":
    sys.exit(main())
