---
id: BR-8
title: Pin actionbook/rust-skills (incl. domain-cli) and run a full code review against them
status: backlog
priority: none
assignee: jpsyx
labels: [chore, tech-debt]
estimate: 13
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-08-11
updated: 2026-08-11
---

# BR-8: Pin actionbook/rust-skills (incl. domain-cli) and run a full code review against them

## Description

This repo pins its agent **development** skills in `skills-lock.json` so every
contributor gets the same guidance regardless of what is installed globally.
Today it pins `leonardomso/rust-skills` — a single `SKILL.md` of idiomatic-Rust
rules.

`actionbook/rust-skills` is a substantially richer set: 25 skills, including
`domain-cli` (directly relevant, since brain *is* a CLI + TUI), plus
`coding-guidelines`, `core-actionbook`, and fourteen focused modules
(`m01-ownership`, `m02-resource`, `m03-mutability`, `m04-zero-cost`,
`m05-type-driven`, `m06-error-handling`, `m07-concurrency`, `m09-domain`,
`m10-performance`, `m11-ecosystem`, `m12-lifecycle`, `m13-domain-error`,
`m14-mental-model`).

Two parts, in order:

1. **Pin them here** so they are part of the repo's contributor toolchain, not
   one machine's global setup.
2. **Then do a full code review of the codebase against them** — the point of
   installing them. `pedantic` + `nursery` clippy already run clean, so the
   value is in what a linter cannot see: ownership and borrowing shape, API
   design, error modeling, domain types, and CLI ergonomics.

The review should **produce findings and follow-up tasks**, not one sprawling
refactor commit. Expect it to split into per-theme tasks once the scope is real.

## Acceptance criteria

- [ ] `actionbook/rust-skills` skills are pinned in `skills-lock.json`, at
      minimum `domain-cli`; decide explicitly whether to take the full set or a
      curated subset, and record the reasoning.
- [ ] Decide what happens to the existing `leonardomso/rust-skills` pin: keep
      both, or replace it. Do not leave two overlapping Rust rule sets pinned
      without a stated reason.
- [ ] `npx skills experimental_install` materializes the new skills cleanly from
      a fresh clone (the materialized dirs stay gitignored).
- [ ] The "Agent dev-skills" section of `AGENTS.md` / `CLAUDE.md` lists the new
      pinned set and says when to invoke each — the docs contract requires this
      in the same change.
- [ ] A written review exists covering, at minimum: `src/cli/` + `src/command/`
      against `domain-cli`; error types and `anyhow` context against
      `m06-error-handling` and `m13-domain-error`; the `src/agent/` frontend trait
      surface against `m05-type-driven` and `m09-domain`; and the sync/server
      concurrency paths against `m07-concurrency`.
- [ ] Findings are triaged into this board as separate tasks, each with its own
      pointers, rather than fixed inline during the review.

## Notes

### Pointers (as of 2026-08-11)

- `skills-lock.json` — the committed pin registry (currently 4 skills:
  `repo-product-manager`, `rust-skills`, `systematic-debugging`,
  `test-driven-development`). Add with
  `npx skills add actionbook/rust-skills@<skill>`; project scope is the default.
  Commit the updated lockfile.
- `AGENTS.md` (`CLAUDE.md` is a symlink to it) — the "Agent dev-skills (pinned
  per-repo, not global)" section documents the pinned set and how to restore it.
  Update it in the same change; the docs contract is not optional here.
- `.agents/`, `.claude/skills/`, `.windsurf/` — gitignored materialized copies,
  the "node_modules" of the `skills` CLI. Never commit them; restore with
  `npx skills experimental_install`.
- **Name collisions to check before adding.** `actionbook/rust-skills` exposes
  its skills as subdirectory names (`domain-cli`, `coding-guidelines`, …), so it
  should not collide with the existing `rust-skills` key — but
  `coding-guidelines` may already exist in the user's *global* registry, and the
  repo pin should win for contributors. Verify what `npx skills add` does on a
  name clash before assuming.
- `docs/architecture.md` — read first to orient the review; it maps every module
  and the data flow. `docs/decisions.md` explains the non-obvious choices, so a
  review finding that contradicts a recorded decision needs to argue with it
  rather than rediscover it.
- `src/cli/` and `src/command/` — the clap surface and per-command handlers, the
  primary target for `domain-cli`. Weigh findings against the house rules already
  in `AGENTS.md`: a flag for every action, an interactive fallback for humans, and
  all color through `src/theme.rs` semantic tokens.
- `src/agent/` — `frontend.rs` (the `AgentFrontend` trait), `registry.rs`, and the
  three adapters (`claude.rs`, `codex.rs`, `opencode.rs`). The most interesting
  surface for `m05-type-driven` / `m09-domain`: how much frontend variation the
  type system enforces versus what convention holds together.
- `src/sync/` and `src/server/` — the concurrency and IO-heavy paths (locks,
  leases, the delivery thread, the watcher) for `m07-concurrency`. `src/sync/`
  also holds the pure/impure split the repo cares about most.
- Error modeling for `m06-error-handling` / `m13-domain-error`: typed errors exist
  in places (`AgentError`, `CsvSyncError`, `ManifestError`) and `anyhow` context
  elsewhere. Recent work found two cases where a `context()` string hid the cause
  from the user, so this theme has known real bite.
- **Related:** BR-3 (in-house Notion/Linear CLIs) is a CLI-design task that
  `domain-cli` should inform. Read this review's CLI findings before starting it.
- **Scope control:** the codebase is ~1,600 tests across 60 binaries with clippy
  `pedantic` + `nursery` clean. Do not re-report what clippy already enforces;
  the review earns its keep only on design-level findings.

### Log

- 2026-08-11 created. Confirmed `actionbook/rust-skills` currently exposes 25
  skills including `domain-cli`; no existing task covers this (checked `tasks/`
  and `archive/` for rust-skills / code-review / domain-cli).
