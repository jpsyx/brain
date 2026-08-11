---
id: BR-9
title: Write the missing second-brain cleanup.sh
status: backlog
priority: none
assignee: jpsyx
labels: [bug, chore]
estimate:
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-08-11
updated: 2026-08-11
---

# BR-9: Write the missing second-brain cleanup.sh

## Description

Three bundled skills instruct the agent to run a cleanup script at the end of
any task that touched the brain:

- `skills/second-brain/SKILL.md:477` — "End of session: clean up tool
  byproducts", plus the mistakes-table row at `:1025`
- `skills/brain-knowledge-capture/SKILL.md:138` and `:224`
- `skills/contacts/SKILL.md:176`

All three call `bash "$BRAIN_ROOT/.agents/skills/second-brain/cleanup.sh"`, and
`second-brain/SKILL.md:15` states that `brain skills sync` installs the skill
"(with its `cleanup.sh`)".

**The script does not exist and never has.** `skills/second-brain/` contains
only `SKILL.md`, so `brain skills sync` installs no script and the documented
path resolves to nothing. Every agent that follows the instruction gets
`No such file or directory`. Because the call sits at the very end of a run,
the failure is easy to miss: the agent either silently skips cleanup or
hand-rolls a `find -delete`, which is exactly what happened on 2026-08-11 (an
agent reorganizing the brain's skill registry hit the missing path and
improvised the deletion instead).

The consequence is the thing the section exists to prevent: byproducts
accumulate, pollute `rg` results, and bloat brain sync. `~/brain` currently
carries `pipeline.json` and `pipeline.sh` at its root, both on the documented
removal list.

## What cleanup.sh is supposed to do

A small, dependency-free shell script whose **pattern list is the documented
source of truth** for "what counts as a tool byproduct in the brain". When a
new artifact type shows up (a new MCP server's session files, a new cache
format), the fix is to add a pattern here rather than delete it one-off.

**Contract** (all of this is already promised by the docs — the script has to
match them, or the docs change in the same commit):

| Behavior | Source |
| --- | --- |
| Invoked as `bash "$BRAIN_ROOT/.agents/skills/second-brain/cleanup.sh"` | all three skills |
| Idempotent — safe to run repeatedly, a no-op when the brain is clean, exit 0 | `second-brain/SKILL.md` |
| `--dry-run` prints what *would* be removed and deletes nothing | `second-brain/SKILL.md` |
| `BRAIN_DIR=/some/path` points it at a non-default brain | `second-brain/SKILL.md` |

**Patterns to remove**, per `second-brain/SKILL.md:1025` and the section body:

- `.DS_Store` (macOS Finder metadata)
- `__pycache__/`, `.pytest_cache/` (Python caches)
- `pipeline.json`, `pipeline.sh` (tool scaffolding at the brain root)
- `*.stats.csv*` (cache files)

**Design constraints worth getting right:**

1. **Do not follow symlinks.** The brain root now contains a skill registry at
   `.agents/skills/` with symlinks pointing into it from `.claude/skills/`,
   `.codex/skills/`, `.opencode/skills/`, and `.config/opencode/skills/`. A
   `find -L` would walk the same trees four times over, and could escape the
   brain entirely via a link that points outside it. Use plain `find` (no
   `-L`) and prune symlinked dirs.
2. **Never delete outside the resolved brain root.** Resolve the root once,
   confirm it looks like a brain (the four PARA buckets are the cheap check
   used elsewhere), and refuse to run if it does not — a script that
   recursively deletes by glob pattern must not run against `/` or `$HOME`
   because of an unset variable.
3. **Skip `node_modules/`.** `~/brain/.opencode/node_modules/` is large and
   not the brain's content; walking it wastes most of the runtime.
4. **Report what it did**, so the caller can state it plainly rather than
   guess: counts per pattern, or an explicit "already clean".
5. **No `+x` needed** — every documented call site invokes it through `bash`.
   If it does ship executable, confirm `brain skills sync` preserves the mode.

## Acceptance criteria

- [ ] `skills/second-brain/cleanup.sh` exists and implements the contract table
      above (invocation, idempotency, `--dry-run`, `BRAIN_DIR`).
- [ ] It removes every pattern in the list, and refuses to run against a path
      that does not look like a brain root.
- [ ] It does not follow symlinks and does not descend into `node_modules/`.
- [ ] `brain skills sync` installs it to
      `<brain root>/.agents/skills/second-brain/cleanup.sh`, so the path the
      three skills document actually resolves. Verify a **root-level**
      non-`SKILL.md` file is copied — the installer is known to copy
      `references/` and `scripts/` subdirectories (see `skills/todo/`), but a
      file at the skill root is untested.
- [ ] Red/green TDD per `AGENTS.md`: a failing test first. Cover at minimum
      that the script is installed alongside `SKILL.md`, that `--dry-run`
      deletes nothing, and that a second consecutive run is a no-op.
- [ ] `bundled_skills_carry_no_personal_data` still passes (the script must
      contain no personal path).
- [ ] `cargo test --release` and `cargo clippy --release --all-targets` green.
- [ ] Docs updated in the same change if any promised behavior is dropped
      rather than implemented.

## Notes

### Pointers (as of 2026-08-11)

- `skills/second-brain/SKILL.md` — the spec. Section "End of session: clean up
  tool byproducts" (~`:470`) is the contract; the mistakes-table row at
  `:1025` carries the canonical pattern list. Read this first; the script has
  to match what is already published, or both change together.
- `skills/second-brain/` — where `cleanup.sh` belongs, next to `SKILL.md`.
- `skills/brain-knowledge-capture/SKILL.md`, `skills/contacts/SKILL.md` — the
  other two callers. Check their wording still holds once the script is real.
- `skills/todo/scripts/`, `skills/todo/references/` — proof that the installer
  copies non-`SKILL.md` files from a skill directory; use it as the reference
  for how a root-level file needs to be declared, if at all.
- `src/` — find the `brain skills sync` render/install path and confirm how it
  enumerates files per skill. This is the one place the task could turn out to
  need a code change rather than just a new script.
- `AGENTS.md` — red/green TDD is the iron law here, plus the clippy bar and
  the bundled-skills privacy test.

### Log

- 2026-08-11 created. Discovered while moving personal skills out of the global
  registry into `~/brain/.agents/skills/`: the documented cleanup call failed,
  and cleanup was done by hand instead. Left untriaged (`backlog` / `none`) for
  the user to prioritize.
