---
id: BR-2
title: Move bundled skills from skills/ to src/core-skills/
status: backlog
priority: none
assignee: jpsyx
labels: [tech-debt, chore]
estimate:
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-08-03
updated: 2026-08-03
---

# BR-2: Move bundled skills from skills/ to src/core-skills/

## Description

The repo's top-level `skills/` directory holds the **product skills brain
bundles into the binary** (`article-summarizer`, `brain-knowledge-capture`,
`contacts`, `second-brain`, `todo`, `triage`) — generic markdown + scripts
embedded via `include_dir!` and later rendered/installed into the shared
agent registry for brain users.

The name `skills/` is misleading: at a repo's top level it reads like the
*agent development skills available to someone working on this repo*, which is
exactly what the **gitignored** `.claude/skills/` / `.agents/` materializations
(pinned in `skills-lock.json`) actually are. The two are unrelated, and the
collision of names causes confusion every time someone opens the tree.

Rename/move the bundled set to **`src/core-skills/`**. Putting it under `src/`
and calling it `core-skills` makes it unambiguous that these are **bundled
markdown assets compiled into the brain binary**, not skills a repo agent can
invoke. "core" also distinguishes them from user-owned extensions/plugins
(`<brain>/.config/...`), which stay where they are.

This is a pure move + reference-update refactor. No skill *content* changes,
no behavior changes: the same six skills must still embed, render, install,
and fan out exactly as before, and `bundled_skills_carry_no_personal_data`
must still pass.

## Acceptance criteria

- [ ] `skills/` (the six bundled product skills, with their `SKILL.md`,
      `references/`, and `scripts/`) is moved to `src/core-skills/` via
      `git mv` so history is preserved.
- [ ] `include_dir!("$CARGO_MANIFEST_DIR/skills")` in `src/skills/embed.rs`
      points at the new `$CARGO_MANIFEST_DIR/src/core-skills` path; the module
      doc comment is updated to name the new location.
- [ ] Existing embed/bundle tests still pass unchanged in intent (the six
      skills embed, `bundled_skills_carry_no_personal_data` stays green). Add a
      RED-first assertion pinning the new source path if one is feasible.
- [ ] All prose references to the bundled-skills path are updated: `CLAUDE.md`
      (`AGENTS.md`) — the `docs/` contract rows and the "product skills … in
      `skills/`" note; `docs/architecture.md` (the `skills/` heading + the
      `include_dir` dependency note); `docs/decisions.md`; `README.md`; and any
      routing note that says bundled skills live under `skills/`.
- [ ] No stale reference to the old top-level `skills/` bundled path remains
      (grep is clean, excluding `.claude/skills/`, `~/global-skills`,
      `agents/skills`, and `skills-lock.json`, which are unrelated).
- [ ] `cargo test --release` and `cargo clippy --release --all-targets` clean;
      crate version bumped in `Cargo.toml` + `Cargo.lock`.

## Notes

Decision recorded up front (from the request): the target is
**`src/core-skills/`**, not a top-level `core-skills/`. Under `src/` + the
"core" qualifier is what makes "compiled-in bundle, not a repo agent skill"
obvious. If moving under `src/` turns out to fight the `include_dir!` /
`CARGO_MANIFEST_DIR` setup in some unforeseen way, fall back to top-level
`core-skills/` and note why — but `src/core-skills/` is the intended home.

### Pointers (as of 2026-08-03)

- `skills/` — the six bundled product skills to move (each a dir with
  `SKILL.md`, some with `references/` and `scripts/`). Use `git mv`.
- `src/skills/embed.rs` — **the one code anchor**:
  `static SKILLS: Dir = include_dir!("$CARGO_MANIFEST_DIR/skills");` plus the
  module doc comment "embedded into the binary from the repo's `skills/` dir".
  Change both. The rest of `src/skills/` (`render`, `install`, `layout`,
  `command`, `plugin`, `extension`) works off `bundled_skills()` and needs no
  path change — but re-read to confirm none hardcodes `skills/`.
- `docs/architecture.md` — the `### skills/` section (~line 345) and the
  `include_dir` dependency note (~line 830).
- `docs/decisions.md` — the "embedding the `skills/` dir" rationale (~line 1055)
  and the embed.rs nomenclature note (~line 1480).
- `CLAUDE.md` / `AGENTS.md` — the docs-contract row for the skill pipeline
  ("bundled skills under `skills/`"), the second-brain-sync row that cites
  `skills/second-brain/SKILL.md`, and the closing note distinguishing product
  skills (`skills/`) from the pinned dev skills. Update all to the new path.
- `README.md` — the "authoritative list is the repo's `skills/<name>/SKILL.md`"
  note (~line 423).
- **Do NOT touch**: `skills-lock.json`, `.claude/skills/`, `.agents/`,
  `.windsurf/` — those are the gitignored *developer* dev-skill
  materializations, a different concept, and the source of the naming
  confusion this task fixes.

### Log

- 2026-08-03 created. Scope: move the six bundled product skills from top-level
  `skills/` to `src/core-skills/`, update the single `include_dir!` anchor and
  all prose references; pure refactor, no content or behavior change.
