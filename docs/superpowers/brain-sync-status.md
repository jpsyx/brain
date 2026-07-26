# Brain sync (Sub-project C) — program status & remaining work

- **Updated:** 2026-07-26
- **Purpose:** the single "where we are / what's left" handoff for the
  A → B → C "make `brain` generic + synced" program. Read this first, then
  `AGENTS.md`, then the phase spec/plan for whatever you pick up.

---

## Status: C1–C5 all shipped and merged to `main`

The full A → B → C program is complete. A (personalization/config) and B (skill
pipeline) shipped earlier. Sub-project C (Backblaze sync) is now done end to end:

| Phase | What it delivered | Where |
|---|---|---|
| **C1** | brain env / brain config split; `brain env {list\|get\|set}`; `root` + `markdown_to_pdf_path` moved to env; legacy pointer auto-migration; `sync` block schema (parse-only). | `src/env/`, `src/paths.rs` |
| **C2** | Sync core: `brain sync [setup\|init\|status\|conflicts\|--push\|--pull]` over `rclone bisync`; keep-both conflicts; bidirectional deletes + `--max-delete`; journal + post-sync verify; `brain check`. | `src/sync/{args,remote,run,verify,journal,setup,command,check,conflicts}.rs` |
| **C3** | id-keyed 3-way CSV merge for `tasks.csv`/`habits.csv` (converges, idempotent; `last_touched` LWW). | `src/sync/{csv_merge,csv_sync}.rs` |
| **C4** | Auto-sync triggers: `notify` watcher + pure debounce, on-start background sync, detached on-exit sync, machine-wide advisory sync lock. | `src/sync/{lock,trigger,watch}.rs`, `src/tui/event_loop/setup.rs` |
| **C5** | Agent-facing conflict resolution + migration: `brain sync conflicts --json`, `brain sync resolve <original>`, `/second-brain cloud-sync` + `/second-brain resolve-conflicts`, hermetic C1-migration test, migration runbook, gated resolve round-trip test, full docs. | `src/sync/{conflicts,command/mod,command/resolve}.rs`, `skills/second-brain/SKILL.md`, `tests/sync_local.rs` |

Design docs: parent spec `specs/2026-07-24-brain-sync-design.md`; per-phase
specs/plans under `specs/` and `plans/`. Rationale in `../decisions.md`.
State at time of writing: `cargo test --release` green (645 tests), `cargo
clippy --release --all-targets` clean.

---

## Remaining work (prioritized; none blocking)

Pick up in roughly this order. Each is a self-contained RED→GREEN TDD slice
unless noted. Update this file (check the box, note the commit) as you land each.

### 1. C3.3 — `last_touched` writer audit  *(natural loose end of C3)*
- [x] Audit every task/habit **writer** so it bumps `last_touched` on **every**
  mutation. If writers don't, the same-field CSV 3-way merge can't resolve
  last-writer-wins accurately (it falls back safely to keep-local + journal, so
  this is correctness-sharpening, not a crash).
- Writers to check: the `todo` skill scripts (`skills/todo/scripts/*.py`,
  incl. `touch_task.py`, `add_task.py`, `mark_done.py`), the second-brain
  `sync.py`, any brain-side Rust that writes the CSVs, and the `/habits` server
  POST path.
- Deliverable: each writer sets `last_touched` to "now" on mutate; a test (pure
  where possible) proving it. Cross-ref `src/sync/csv_merge.rs` for how the
  field is consumed.
- Landed in `7fbb60d` (`fix: stamp task csv mutations`). Notes: `habits.csv`
  now carries `last_touched` parity with `tasks.csv`; bundled task/habit
  mutators stamp rows via `_csvlib.touch_row`, legacy habit files gain the
  column on mutation, `apply_sync_rules.py --fix` stamps rows it repairs, and
  the Rust habit loader preserves the timestamp when present. Validation:
  `cargo test --release` green (646 unit tests plus integration suite; watcher
  timing test still ignored by design) and `cargo clippy --release --all-targets`
  clean.

### 2. C3.4 — extend `brain check` to diff pending CSV rows
- [ ] `brain check` today runs a dry-run `rclone bisync` to report pending
  changes — but the two CSVs are **excluded** from bisync (C3), so `check`
  currently misses pending CSV edits. Add a CSV-diff pass to `check` that
  reports pending `tasks.csv`/`habits.csv` rows (added/changed/deleted vs. the
  cached baseline) alongside the bisync file-lane report.
- Files: `src/sync/check.rs` (report), reuse `src/sync/csv_sync.rs` baseline +
  `src/sync/csv_merge.rs` diff logic. Keep the pure/impure split; theme output.

### 3. C4 hardening — sync-lock heartbeat  *(optional; deferred in C4)*
- [ ] The advisory sync lock (`~/.cache/brain/sync/sync.lock`) has one residual
  edge: a SIGKILLed holder whose PID gets recycled to a live unrelated process
  wedges the lock until the file is removed by hand. A heartbeat (refresh the
  lockfile mtime during a sync + a short staleness cap alongside the existing
  PID-liveness check) closes it. Deferred in C4 as "not worth the extra thread
  for now" — do it only if the wedge is actually hit.
- Files: `src/sync/lock.rs` (pure `is_stale` already takes an age cap).

### 4. C5 optional follow-ups  *(from the C5 final adversarial review)*
- [ ] Test: `brain sync resolve` with **multiple originals** in one call
  (the skill batches; `resolve_many` only loops today — no integration test).
- [ ] Test: resolve of a conflict copy in a **nested subdir**.
- [ ] Test: the `conflicts --json` `modified: null` / `bytes: null` degraded
  path against a real unreadable-metadata case.
- [ ] Consistency: render the **non-JSON** `brain sync conflicts` list through
  `group_conflicts` (strict parse) too, so it matches `--json` (today it uses
  the looser `list_conflicts` heuristic; no safety impact, cosmetic).

### 5. §19 deferred backlog  *(parent spec §19 + C4 §11 — revisit as wanted)*
- [ ] `--check-access` marker-file guard (needs create/maintain a marker in the
  brain root on setup).
- [ ] `rclone crypt` (zero-knowledge client-side encryption) — a clean seam was
  left; layering it must not change the `brain sync` surface. Passphrase escrow
  is the user's responsibility.
- [ ] Native-Rust `mark_done.py` (remove the Python coupling from the
  completion path: mutate the CSV + spawn the next recurrence in Rust).
- [ ] Inbound webhook endpoints (`src/server/routes/` is structured for one
  route module + one `routes/mod.rs` line per endpoint).
- [ ] C4 idle-pull timer (`sync.idle_pull_secs`) and/or a standalone always-on
  sync daemon (reuses the `sync_once`-under-lock core).

---

## How to work in this repo (must-follow)

Read `AGENTS.md` (== `CLAUDE.md`) in full first. The non-negotiables:

- **Red/green TDD is the iron law.** Write the smallest failing test, RUN it,
  watch a real assertion fail, THEN write the code that makes it pass. Push
  decision logic into **pure functions** and test those; keep the
  `Command`/`/dev/tty`/`notify` shells thin.
- **No `unsafe`** (`unsafe_code = "forbid"`; note `std::env::set_var` is unsafe
  in this edition — never mutate global env in tests; inject paths instead).
- **Clippy `pedantic` + `nursery` clean** — add zero new warnings
  (`cargo clippy --release --all-targets`).
- **`cargo test --release` stays green** (runs in ~1s).
- **Update `docs/` in the SAME change** per the docs-contract table in
  `AGENTS.md`. Docs are the source of truth for *what/why*.
- **One module per file; ~400 production lines is the split smell** (inline
  `#[cfg(test)]` doesn't count). Preserve public paths when splitting
  (re-export from `mod.rs`).
- **The repo is 100% public + generic.** No bucket names, hosts, emails, org
  names, or private paths anywhere. `bundled_skills_carry_no_personal_data`
  guards bundled skills; docs/tests are unguarded — keep them generic by hand.
- **No `.difit/` files** — this repo keeps none; rationale goes in
  `docs/decisions.md`.
- **All CLI color via `src/theme.rs` `Theme` tokens**; every action has a flag
  path AND a human-friendly interactive fallback when a value is omitted.

### Cadence
- **Small, well-scoped slice** (most of the above): go straight to RED→GREEN,
  update docs, commit.
- **Larger item** (e.g. C3.4 or a §19 feature): write a short spec under
  `docs/superpowers/specs/`, then a plan under `docs/superpowers/plans/`, then
  build (the earlier C-phase specs/plans are the template).

### Gotcha learned the hard way
If you fan work out to sub-processes/agents that share this working directory,
a stray `git checkout` in one can move your `HEAD` to another branch. **Always
`git rev-parse --abbrev-ref HEAD` before committing/merging**, or use isolated
git worktrees for parallel work.

### After each item is done
1. `cargo test --release` green + `cargo clippy --release --all-targets` clean.
2. Docs updated in the same change (per the contract table).
3. Commit (branch per Gitflow: `feat/…`, `fix/…`, `test/…`, `refactor/…`;
   short slug). Merge to `main` when the slice is complete and green.
4. **Update this file**: check the box, note the commit, adjust priorities.
