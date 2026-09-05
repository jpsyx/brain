---
id: BR-20
title: Never leave a conflict unresolved; force LWW and record it in a rollback ledger
status: backlog
priority: high
assignee: jpsyx
labels: [feature, enhancement, sync, bug]
estimate: 8
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-09-05
updated: 2026-09-05
---

# BR-20: Never leave a conflict unresolved; force LWW and record it in a rollback ledger

## Description

Today a `brain sync` that hits a same-file conflict ends with the conflict
*preserved*, not *resolved*: rclone keeps the loser under
`--conflict-loser pathname`, brain renames it to
`name (conflict <host> <date>).ext`, both naming forms are bisync excludes, and
the run is reported as `NeedsAttention`. Nothing ever converges the two copies.
`brain sync resolve` exists but is opt-in and purely a deleter, so the copies
accumulate. On the author's own workspace there are currently **44 friendly
conflict copies and 5 raw markers** sitting in `~/brain`, some three weeks old,
and **36 of the 44 are byte-identical to the file they "conflict" with**.

That is a git-shaped UX in a place where git's tradeoff is wrong. A second brain
is not a source tree: the user is a single person across their own machines,
there is no review step, and an unresolved copy is not a decision deferred —
it is silent data rot. Brain must always land on one canonical file.

**The rule.** Every conflict Brain detects is resolved before the sync run is
reported complete. Resolution is tiered:

1. **Structured / semantic merge** where a schema-aware merge exists and
   succeeds (today: the id-keyed UUID 3-way merge for `tasks/tasks.csv` and
   `tasks/habits.csv` in `src/sync/csv_merge/`). Not a forced choice; nothing
   is logged to the ledger.
2. **Convergence with no loss** where the two sides are provably the same
   content (equal bytes, or equal after a normalization the file type allows).
   Also not a forced choice; nothing is logged.
3. **`lww-register` (forced fallback).** For everything else, the most recent
   edit wins and the loser is discarded from the workspace. This is a genuine
   Last-Writer-Wins Register in the CRDT sense: one register per path, ordered
   by a timestamp with a deterministic total-order tiebreak so every machine
   independently picks the same winner and the result converges. **Every
   tier-3 resolution is written to the ledger**, because it is the only tier
   that can lose someone's edit.

The workspace must contain **zero** `(conflict …)` copies and zero
`__brainconflict__` markers after a completed sync. `Outcome::Clean` becomes an
honest claim rather than "nothing conflicted."

**Two measured rclone constraints the implementation must respect** (verified
against rclone v1.75.0 on 2026-09-05, during the sync investigation):

- **Do not reach for `--conflict-loser delete`.** It resolves by *deleting the
  loser with no copy anywhere*, and rclone still reports `Bisync successful`, so
  `verify::classify` would journal `Outcome::Clean` on a run that silently
  destroyed an edit — the exact opposite of this task's intent, and it would
  remove the very bytes the ledger exists to preserve. Brain must keep
  `--conflict-loser pathname`, **record** the loser, and delete it itself.
- **rclone has a "no winner" state, and it renames *both* sides.** With
  `--conflict-resolve newer` and equal mtimes, rclone logs `Winner cannot be
  determined as times are equal` → `A winner could not be determined`, then
  suffixes *both* files (`__brainconflict__1` and `__brainconflict__2`) and the
  canonical name disappears from both sides. The tier ladder must handle this as
  a first-class case: it is both a source of the recurring self-conflict loop and
  precisely where `lww-register` has no timestamp to order by, so the
  deterministic tiebreak (not side ordering) is load-bearing rather than
  defensive.

**The ledger.** A tier-3 resolution is a forced, possibly lossy decision, so it
must be reversible. Brain keeps an append-only SQLite ledger at
`<brain-root>/.conflict-resolutions.sqlite` recording enough to reconstruct and
roll back the discarded side. Append-only means the table is a
**grow-only set (G-Set)** keyed on `resolution_id`, which is why it can be
reconciled across machines by plain union with no conflict of its own.

Because a SQLite file in the synced root would itself be a whole-file
keep-both conflict, it must be a bisync **exclude** reconciled out-of-band by
union merge, exactly as `src/sync/csv_sync/` already does for the two CSVs.
(Assumption stated rather than asked: the user wants the ledger in the brain
root, so it should be portable across their machines. If it should instead be
machine-local, it moves to `<workspace-cache>/sync/` and the union lane is
dropped — everything else in this task is unchanged.)

Proposed schema (one row per forced resolution):

```sql
CREATE TABLE conflict_resolutions (
  resolution_id     TEXT PRIMARY KEY,   -- uuid; the G-Set key
  workspace_id      TEXT NOT NULL,      -- selected workspace uuid
  resolved_at       TEXT NOT NULL,      -- rfc3339 utc, when brain decided
  sync_run_started  TEXT,               -- joins the sync journal run
  relative_path     TEXT NOT NULL,      -- canonical path inside the root
  strategy          TEXT NOT NULL,      -- 'lww-register' (the only forced tier)
  tiebreak          TEXT,               -- what broke a timestamp tie, when one did
  winner_side       TEXT NOT NULL,      -- 'local' | 'remote'
  winner_machine    TEXT,               -- stable machine id, when known
  winner_mtime      TEXT NOT NULL,
  winner_size       INTEGER NOT NULL,
  winner_sha256     TEXT NOT NULL,
  loser_side        TEXT NOT NULL,
  loser_machine     TEXT,
  loser_mtime       TEXT NOT NULL,
  loser_size        INTEGER NOT NULL,
  loser_sha256      TEXT NOT NULL,
  content_kind      TEXT NOT NULL,      -- 'text' | 'binary'
  loser_text        TEXT,               -- full losing text when text and under the cap
  loser_blob_path   TEXT,               -- spill location when text is over the cap or binary
  conflict_hunks    TEXT,               -- json: [{winner_start,winner_end,loser_start,loser_end}]
  conflict_diff     TEXT,               -- json or unified diff of just the differing hunks
  rolled_back_at    TEXT,               -- set by `brain sync rollback`, never overwritten
  brain_version     TEXT NOT NULL,
  notes             TEXT
);
```

`conflict_hunks` / `conflict_diff` are what satisfy "the lines that had
conflicts, the exact text": for a text file, record the line ranges that
actually differ (not the whole file) plus the losing text verbatim, so a human
or an agent can see precisely what was dropped. Binary and oversized losers
spill to a content-addressed blob under
`<brain-root>/.conflict-resolutions/<sha256>` (also excluded from bisync,
also unioned) and the row points at it.

A rollback surface is part of the deliverable, not a follow-up: without a way
to *use* the ledger, the ledger is only a promise. `brain sync rollback
<resolution_id>` (and a `--dry-run`) restores the losing content to the
canonical path, stamps `rolled_back_at`, and never mutates any other column.

**Note on BR-11.** [BR-11](BR-11-interactive-sync-conflict-resolution.md) asks
for the opposite default: prompt the user per conflict and hand the remainder to
an LLM. This task supersedes BR-11's *default*. Brain resolves automatically and
silently; asking a human is not a step in the happy path. BR-11's flow should be
rescoped to an explicit, opt-in review command over the ledger (an LLM pass that
proposes a semantic merge for rows already resolved by `lww-register`, which the
ledger makes safe because the losing bytes are still recoverable). Decide that
rescope before starting either one.

**Note on a prerequisite that outranks this task.** Brain's sync lock is
**machine-local** (`WorkspacePaths::sync_lock()` resolves under
`~/.cache/brain/workspaces/<uuid>/`), so two machines can run the CSV lane
concurrently, and that lane is an uncoordinated read-modify-write on one shared
remote object (`csv_sync/transport.rs` `batch_download` → merge →
`batch_upload`) with no compare-and-swap. A lost update there feeds
`csv_merge/merge.rs`'s `(Some(base), Some(side == base), None)` arm, which reads
"present in my baseline, unchanged locally, absent remotely" as a remote delete
and **drops the row locally**. That is silent task loss, and no amount of
conflict-resolution policy in this task prevents it. It needs its own task
(a remote lease, or single-writer-per-object state files with explicit
tombstones) and it should be treated as higher priority than this one.

**Note on the false-positive conflicts.** The largest source of tier-3
resolutions today is not real divergence — it is Brain conflicting with itself.
Of 1,378 conflict renames in the 2026-09-05 investigation, 1,376 were the five
Brain-installed lifecycle artifacts (`.opencode/plugins/brain.js` 430,
`.brain/hooks/agent_session_stop_hook.py` 430,
`.brain/hooks/agent_session_start_hook.py` 430, `.codex/hooks.json` 43,
`.claude/settings.json` 43), and 36 of the 44 surviving conflict copies are
byte-identical to the file they conflict with.

The mechanism is *not* that those files are machine-local — they are portable
and their writers are already byte-idempotent. It is that (a) rclone's own
conflict rename evicts the canonical path from **both** saved baselines, so the
path reads as "new on both sides" until the two sides converge, and (b) the
five-minute automatic reconcile runs `--conflict-resolve path2`, so the stale
remote copy wins that comparison. Add brain re-rendering files inside the root
*during its own bisync* (which fails rclone's listing validation, is
misclassified as a missing baseline, and triggers a `path1`-wins resync), and
the episode sustains itself.

Fixing those at the source is separate work and must land **first**, so this
task's ledger records real user divergence rather than thousands of
Brain-vs-Brain rows. Until it does, this task would faithfully log Brain's own
noise.

## Acceptance criteria

- [ ] A completed `brain sync` leaves zero `*(conflict *)*` copies and zero `*.__brainconflict__*` markers anywhere under the root, on both the local root and the remote.
- [ ] Every detected conflict is resolved through the tier ladder (semantic merge → provable convergence → `lww-register`), and the ladder is a pure, unit-tested decision function.
- [ ] The `lww-register` fallback is convergent: two machines resolving the same pair independently pick the same winner, and re-resolving an already-resolved pair is a no-op. Both are asserted as tests, mirroring `csv_merge`'s convergence/idempotency tests.
- [ ] Timestamp ties resolve by an explicit deterministic total order (never by side ordering, never by which machine ran the sync), and the tiebreak used is recorded in `tiebreak`.
- [ ] rclone's "a winner could not be determined" state (equal mtimes, both sides suffixed, canonical name gone from both) is handled explicitly by the ladder and covered by a test; `--conflict-loser` stays `pathname` and brain performs the deletion itself after recording.
- [ ] Tier 1 and tier 2 resolutions write **no** ledger row; every tier-3 resolution writes exactly one.
- [ ] The ledger is created on first forced resolution, is append-only in practice (no `UPDATE` except `rolled_back_at`, no `DELETE`), and survives a crash mid-run without a partial row.
- [ ] For a text loser, the row carries the verbatim losing text and the differing line ranges — not the whole-file diff — and a binary or oversized loser spills to a content-addressed blob the row references.
- [ ] The losing bytes are recoverable: `brain sync rollback <resolution_id>` restores them to the canonical path, supports `--dry-run`, stamps `rolled_back_at`, and refuses cleanly when the blob or text is missing.
- [ ] `brain sync conflicts` (and `--json`) reports resolution *history* from the ledger rather than an open-conflict list that can no longer exist; the JSON shape is documented.
- [ ] The ledger and its blob directory are bisync excludes and are reconciled out-of-band by a union merge on `resolution_id` that is convergent and idempotent (or, if the machine-local variant is chosen instead, that decision is recorded in `docs/decisions.md`).
- [ ] Every forced resolution is narrated in the sync output with the theme tokens (which path, which side won, why) and summarized in the sync journal note, so a lossy decision is never silent.
- [ ] The 44 existing conflict copies and 5 markers in an already-affected workspace are migrated: each is either converged or turned into a ledger row, and the workspace ends clean.
- [ ] `docs/features.md`, `docs/data-model.md`, `docs/architecture.md`, `docs/integrations.md`, `docs/config.md`, and `docs/decisions.md` are updated in the same change, including a decision record explaining why brain forces LWW where git asks a human.
- [ ] BR-11 is explicitly rescoped or closed as superseded, so the two do not ship contradictory defaults.

## Notes

### Pointers (as of 2026-09-05)

High-level guide, not a plan; verify against the tree when the task starts.

- `src/sync/conflicts/mod.rs` — conflict-copy naming, marker parsing, `rename_markers`, `list_conflicts`, `group_conflicts`. The resolution ladder replaces `rename_markers` as the post-pass; keep the parsers, they are still needed to migrate the existing copies.
- `src/sync/args.rs` — `bisync_args` sets `--conflict-resolve`/`--conflict-loser pathname`/`--conflict-suffix`, and `EXCLUDES` holds the two conflict-name patterns. Both the loser policy and the excludes are load-bearing here; changing them is the crux of the task, not a detail.
- `src/sync/command/mod.rs::sync_once` — the orchestration order (rclone → rename markers → verify → CSV lane → journal). The resolution pass slots in where `rename_markers` is today, before `verify::classify`.
- `src/sync/verify.rs` — `classify` currently returns `NeedsAttention` whenever any conflict copy was created. That contract inverts: a resolved conflict is clean, and only a resolution that *failed* is `NeedsAttention`.
- `src/sync/csv_merge/` and `src/sync/csv_sync/` — the existing tier-1 lane and the reference implementation for everything this task needs: a machine-local baseline, a pure convergent merge, whole-operation preflight, and out-of-band reconciliation of a bisync-excluded file. Read `csv_merge/mod.rs`'s convergence/idempotency tests before writing the LWW tests.
- `src/sync/command/resolve.rs` and `resolve_remote.rs` — today's manual deleter, local and remote halves. The remote half is directly reusable: a forced resolution must delete the remote loser too, and `resolve_remote` already knows both naming forms.
- `src/sync/journal.rs` — the machine-local SQLite sync journal (`sync_runs`). Follow its open/migrate/insert shape for the new ledger rather than inventing a second SQLite idiom; `sync_run_started` is the join key back to it.
- `src/sync/command/mod.rs::hostname` — the only "who" Brain has today, and it is unstable (this machine has appeared as `Mac`, `MacBook-Pro-10`, and `Avandar-MacBook-Pro`). `winner_machine`/`loser_machine` need a durable per-machine id; if none exists yet, either add one or leave the columns null rather than recording a hostname that will not match next month.
- `src/theme.rs` — every line the resolution pass prints goes through the semantic tokens; a forced lossy decision is `warning`, a converged one is `muted`.
- `docs/decisions.md` §C2 ("Why a same-file conflict is 'keep both'") and §C3 (the CSV merge) — C2 is the decision this task reverses; write the new record as an explicit supersession of it, keeping its reasoning about why discarding an edit is dangerous and explaining how the ledger discharges that risk.
- `docs/testing.md` — the red/green pure-function strategy this repo requires; the ladder, the LWW ordering, the hunk extraction, and the union merge are all pure and must be tested before any production code.

### Log

- 2026-09-05 created, out of the sync accuracy/performance investigation. Related: [BR-11](BR-11-interactive-sync-conflict-resolution.md) (supersedes its default), [BR-6](BR-6-reuse-one-rclone-process-per-sync.md) (sync performance).
