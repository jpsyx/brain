---
id: BR-5
title: Explain namespaces and tags during setup, and document what `mit` means
status: backlog
priority: none
assignee: jpsyx
labels: [enhancement]
estimate:
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-08-08
updated: 2026-08-08
---

# BR-5: Explain namespaces and tags during setup, and document what `mit` means

## Description

The namespace and tag checklists currently drop the user straight into a bare
list of pre-checked items (`work`, `personal` for namespaces; `mit`,
`personal`, `work` for tags) with no explanation. A new user has no way to know
what a namespace is for, what a tag does, that the shipped defaults are only
suggestions they can uncheck or replace entirely, or what `mit` stands for.

Improve the setup experience for both `namespaces` and `tags`:

- Add intro text to each checklist explaining what the concept is for
  (namespaces are the `<namespace>__<outcome>` life-buckets that group
  projects; tags label individual tasks and drive their emoji/label display).
- Make it explicit that none of the pre-checked defaults are required: the user
  can uncheck all of them, add their own, and brain works fine either way.
- Explain `mit` ("Most Important Task") wherever it appears, so the shipped
  default is self-describing rather than an unexplained acronym.

Keep the intro text generic and personal-data-free (bundled-skill/core rules):
it explains the mechanism, not this user's taxonomy.

## Acceptance criteria

- [ ] The namespace checklist shows intro/help text explaining what a namespace is and how it is used in project slugs.
- [ ] The tag checklist shows intro/help text explaining what task tags are and how they affect display.
- [ ] Both checklists state that the pre-checked defaults are suggestions, not requirements, and that the set can be emptied or fully replaced.
- [ ] `mit` is expanded to its meaning (Most Important Task) in the setup surface and in the shipped default tag style / docs, so it is never presented as a bare acronym.
- [ ] The interactive checklist still fits and renders correctly at small terminal sizes, and the pure state machine keeps its existing behavior (all items start checked; `a` creates; Enter confirms; Esc cancels).
- [ ] The explanatory copy is covered by tests on a pure function, not by asserting on `/dev/tty` output.
- [ ] `brain config set namespaces` / `brain config set tags` and first-run onboarding both show the new copy (same code path, not duplicated strings).
- [ ] Docs updated per the docs contract: `docs/config.md` (namespace/tag checklist + tag-style defaults) and `docs/data-model.md` if the model changes.

## Notes

### Pointers (as of 2026-08-08)

High-level guide to where and how to complete this, not a detailed plan
(references drift before the task is picked up).

- `src/personalization/checklist/mod.rs` — the pure `Checklist` state machine (title, items, cursor, create mode). Intro/help copy most likely belongs here as pure data on the widget so it can be unit-tested, rather than in the render shell.
- `src/personalization/checklist/run.rs` — the thin ratatui `/dev/tty` shell and `draw`. The layout currently splits into a list block plus a 4-line footer; an intro block needs a third constraint and must degrade gracefully on short terminals.
- `src/personalization/command.rs` — the two call sites (`checklist::choose("Project namespaces", …)` and `checklist::choose("Task tags", …)`); the natural place to pass per-checklist intro copy so both onboarding and `brain config set` share one string.
- `src/personalization/namespaces.rs` — `default_namespaces()` (`work`, `personal`), slug rules, and the `__` project-slug separator that the namespace explanation should describe.
- `src/personalization/tags.rs` — `default_styles()` where `("mit", TagStyle::new("❗", "MIT"))` lives, plus `default_tag_names()`. Expanding `mit`'s meaning likely touches this table and its tests.
- `src/personalization/onboarding.rs` — first-run flow that seeds personalization; confirm the new copy appears on the startup path too, and that it still no-ops without a terminal.
- `docs/config.md` — the personalization schema, namespace/tag checklist, and tag-style defaults section that the docs contract requires updating in the same change.
- House rules: terminal output aesthetics are non-negotiable (theme tokens, human-friendly interactive fallback), and core text must stay generic — no personal taxonomy baked in.

### Log

- 2026-08-08 created.
