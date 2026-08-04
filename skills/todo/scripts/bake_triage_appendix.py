#!/usr/bin/env python3
"""Bake caller-supplied optional markdown into an agenda document.

The agenda and content paths are explicit caller inputs; this script never
discovers content or infers a home-directory workspace. Caller content is
wrapped in one generic marked section so reruns replace it idempotently.

The source's leading H1 is dropped and its remaining headings are demoted so
the generic wrapper remains the only level-two section boundary. Re-running
with new caller content replaces the prior marked section and never duplicates
it.

This script only edits the explicit markdown files. Any rendering or other
post-processing remains the caller's responsibility.

Usage:
    bake_triage_appendix.py --agenda <agenda.md> --content <content.md>
"""
import argparse
import re
import sys
from pathlib import Path
from typing import Optional

APPENDIX_HEADING = "## Appendix <!-- brain:optional-content -->"

_ATX_RE = re.compile(r"^(#{1,6})(\s.*)$")


def _strip_leading_h1(text: str) -> str:
    """Drop a leading `# Title` line (the source's own document title) so it
    doesn't double up under the appendix's `## …` heading. Only the first
    non-blank line, and only if it's an H1."""
    lines = text.splitlines()
    for i, ln in enumerate(lines):
        if ln.strip() == "":
            continue
        if re.match(r"^#\s", ln):
            del lines[i]
            # also swallow one immediately-following blank line
            if i < len(lines) and lines[i].strip() == "":
                del lines[i]
        break
    return "\n".join(lines)


def _demote_headings(text: str) -> str:
    """Demote every ATX heading so the deepest it can sit is `###`. A source
    `##` or `#` becomes `###`; `###` becomes `####`; etc. (capped at 6). This
    guarantees the appendix body contains no `## ` line, so the only
    section-level headings in it are the two this script adds."""
    out = []
    for ln in text.splitlines():
        m = _ATX_RE.match(ln)
        if m:
            level = min(6, max(3, len(m.group(1)) + 1))
            ln = "#" * level + m.group(2)
        out.append(ln)
    return "\n".join(out)


def _prep(path_str: Optional[str]) -> Optional[str]:
    if not path_str:
        return None
    p = Path(path_str).expanduser()
    if not p.is_file():
        return None
    return _demote_headings(_strip_leading_h1(p.read_text())).strip()


def assemble(content_path: str) -> Optional[str]:
    """Wrap one caller-supplied markdown source in the generic boundary."""
    content = _prep(content_path)
    if content is None:
        return None
    return f"{APPENDIX_HEADING}\n\n{content}\n"


def strip_existing_appendix(agenda_text: str) -> str:
    """Return the agenda text with any prior marked optional section removed."""
    lines = agenda_text.splitlines()
    for i, ln in enumerate(lines):
        if ln == APPENDIX_HEADING:
            return "\n".join(lines[:i]).rstrip()
    return agenda_text.rstrip()


def inject(agenda: Path, content: Path) -> bool:
    """Replace the marked optional section with explicit caller content."""
    if not agenda.is_file() or not content.is_file():
        return False
    body = strip_existing_appendix(agenda.read_text())
    optional = assemble(str(content))
    if optional is None:
        return False
    combined = body + "\n\n" + optional
    agenda.write_text(combined)
    return True


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Bake caller-supplied optional markdown into an agenda."
    )
    parser.add_argument("--agenda", required=True, help="Agenda markdown to update.")
    parser.add_argument(
        "--content", required=True, help="Optional markdown supplied by the caller."
    )
    args = parser.parse_args()

    agenda = Path(args.agenda).expanduser()
    content = Path(args.content).expanduser()
    if inject(agenda, content):
        print(f"[bake_triage_appendix] injected optional content into {agenda}")
    else:
        print("[bake_triage_appendix] nothing injected (agenda or caller content is missing)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
