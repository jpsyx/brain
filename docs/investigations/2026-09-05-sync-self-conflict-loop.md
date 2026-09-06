# The self-conflict loop

**Investigation date:** 2026-09-05
**Scope:** `brain sync` accuracy, conflict resolution, and speed
**Status:** diagnosis complete; no code changed

## Verdict

Brain's sync engine is not broken. Brain *trips* it by writing into the synced
root during its own run, *mis-diagnoses* the trip as a missing baseline, and
then "recovers" with a resync that destroys the losing side outright. Every
conflict observed in three weeks of logs is downstream of that.

**No re-architecture is required.** Every failure reproduced is Brain
misconfiguring or racing a correct engine. One item is genuinely structural,
and it is not the conflict machinery: Brain's sync lock is *machine-local*, so
two machines run the task-CSV read-modify-write concurrently against a store
with no compare-and-swap.

## Headline numbers

| Measure | Value |
| --- | --- |
| Conflict copies created by `pull` runs | 1,766 (1,525 runs) |
| Conflict copies created by `push` runs | 0 (1,370 runs) |
| Conflict copies created by `both` runs | 27 (42 runs) |
| Conflict renames that were Brain's own 5 lifecycle artifacts | 1,376 of 1,378 |
| Surviving conflict copies byte-identical to their original | 36 of 44 |
| Logs containing `cannot find prior`, the error Brain believes it handles | 0 of ~180 |
| Logs containing `path1 and path2 are out of sync` | 4 |

Evidence base: `journal.db` (2,939 runs), roughly 180 process logs in `/tmp`,
the live bisync baselines, a full `rclone lsf -R` of the bucket (3,887
objects), and reproductions against rclone v1.75.0 on throwaway path pairs.

## Is the sync architecture documented?

Yes, and unusually well. The transport, the pipeline order, the identity gate,
the CSV merge and its convergence proofs are all written down:

- `docs/architecture.md`, section `sync/`
- `docs/decisions.md`, sections C2/§19, C3, C4, C5
- `docs/integrations.md`, `docs/data-model.md`, `docs/features.md`
- eight design specs under `docs/superpowers/specs/2026-07-*brain-sync*`

Two things are missing, and the bugs live in exactly that gap:

1. No document states the invariant *"a machine must never conflict with its
   own change."* Without a named invariant there is nothing to test against.
2. Nothing classifies which in-root paths are portable content versus
   per-machine generated output. Without that classification, every new
   artifact Brain installs into the root is a coin flip.

## Chain of causation

Numbered because it is a real sequence: each step is the precondition for the
next. Steps 1 to 4 fired in production at 10:47 on 2026-09-05 with nothing
crashed.

### 1. Brain rewrites 33 files inside the synced root, mid-sync

`src/skills/install.rs` (`write_built_to`) calls `fs::remove_dir_all` on
`<root>/.agents/skills/<name>` and rewrites every rendered file, on every TUI
launch, config mutation, and version change. Nothing coordinates that with the
sync lock. rclone had already built its listings 15 seconds earlier.

```
/tmp/2026-09-05T10:47:27...-94371.log   (direction=pull)

10:47:28  INFO : Building Path1 and Path2 listings
10:47:30  INFO : Path1 checking for diffs
10:47:45.76      <- brain re-renders .agents/skills/** under the root
10:47:47  ERROR: Modtime not equal in listing. Path1: 14:47:45.762...   (x33)
10:47:47  ERROR: Bisync critical error: path1 and path2 are out of sync
```

### 2. Brain mislabels that abort as a missing baseline

`parse_outcome` in `src/sync/run/mod.rs` classifies by substring. rclone's
message is `path1 and path2 are out of sync, run --resync to recover`, which
contains the literal Brain matches for a *missing prior listing*. A
listing-*validation* failure is therefore handled as a listing-*absence*
failure.

```rust
} else if lc.contains("cannot find prior")      // 0 log hits, ever
    || lc.contains("must run --resync")
    || lc.contains("run --resync")              // matches "...out of sync, run --resync to recover"
{
    Some(AbortKind::PriorListingMissing)        // -> should_auto_resync() == true
}
```

The string Brain intends to catch appears in zero of roughly 180 logs; the
out-of-sync error appears in four. Of those four, three were already
`direction=resync` (so `should_auto_resync` correctly declined) and one was the
`pull` at 10:47, which fired the resync.

### 3. The "recovery" is a resync in which local wins and the loser is destroyed

Brain passes a bare `--resync` (`src/sync/args.rs`). rclone documents that as
`--resync-mode path1`, and `--resync-mode` appears nowhere in `src/`.
Critically, `--conflict-loser pathname` is **not honored during a resync** and
`--conflict-resolve newer` is ignored: the remote-newer side is overwritten
with no copy kept on either side.

```
before   path1/doc.txt = "LOCAL-older"
         path2/doc.txt = "REMOTE-NEWER-IMPORTANT"

$ rclone bisync ... --conflict-resolve newer --conflict-loser pathname --resync

after    path1/doc.txt = "LOCAL-older"
         path2/doc.txt = "LOCAL-older"
         no .__brainconflict__ copy anywhere; remote content destroyed

with --resync-mode newer:  both sides = "REMOTE-NEWER-IMPORTANT"   (correct)
```

The journal note for that run reads only *"auto-resumed after interrupted
baseline."* It never says local overwrote remote.

### 4. rclone's own conflict rename evicts the canonical path from both baselines

This one is inherent rclone behavior, not Brain's excludes; it reproduces with
no `--exclude` flags at all. After one keep-both rename, the saved listings
hold the *marker* and lose the *canonical name*. The path then reads as "new on
both sides" on every subsequent run until the two sides hold identical bytes
again.

```
~/.cache/brain/workspaces/<uuid>/sync/bisync/*.lst

path1.lst / path2.lst both contain:
  .claude/settings.json.__brainconflict__1
  .codex/hooks.json.__brainconflict__1
  .opencode/plugins/brain.js.__brainconflict__1

neither contains:
  .claude/settings.json          <- present on both sides on disk
  .codex/hooks.json
  .opencode/plugins/brain.js
```

### 5. The five-minute automatic reconcile hands the win to the stale remote

`src/sync/periodic.rs` fires `Direction::Pull` every five minutes from every
open shell, and `args::bisync_args` maps `Pull` to `--conflict-resolve path2`.
For a path in state 4, that resolves in favor of whatever the remote happens to
be holding and demotes the local copy to a conflict copy. The episode then
sustains itself until the two sides converge.

```
journal.db / sync_runs, unattended overnight 2026-09-04

started           direction  outcome           transferred  conflicts
2026-09-04T02:49  pull       needs_attention   5            5
2026-09-04T02:44  pull       needs_attention   5            5
2026-09-04T02:39  pull       needs_attention   5            5
2026-09-04T02:34  pull       needs_attention   5            5
2026-09-04T02:29  pull       needs_attention   5            5
   ... eleven consecutive runs, every five minutes, all night ...
```

That is the reported symptom verbatim: the only machine that touched the
workspace all day, reporting conflicts, against itself, on a timer.

### Why it is episodic, not permanent

The loop self-heals in one run the moment the two sides hold the same bytes
(rclone logs `Files are equal! Skipping`). Reproduced:

```
RUN 2: Path1/Path2 "File is new - x.txt" -> "Files are equal! Skipping: x.txt"  -> x.txt back in lst
RUN 3: "No changes found"
```

432 of 1,489 `pull` runs that reached the diff phase produced conflict renames
(29 percent). It is episodic, and each episode ends when the sides converge or
when an auto-resync forces `remote := local`.

## Which files actually conflicted

All 1,378 `Renaming Path1 copy` events across every log:

| Path | Renames |
| --- | --- |
| `.opencode/plugins/brain.js` | 430 |
| `.brain/hooks/agent_session_stop_hook.py` | 430 |
| `.brain/hooks/agent_session_start_hook.py` | 430 |
| `.codex/hooks.json` | 43 |
| `.claude/settings.json` | 43 |
| `.agents/skills/email-triage/config/*` | 2 |

The mechanism is **not** that these files are machine-local. They are portable
by construction (they reference `$BRAIN_ROOT` / `$CLAUDE_PROJECT_DIR`, never an
absolute machine path), they carry no version string, and the local hook is
md5-identical to `scripts/agent_session_start_hook.py` in the repo. Their
writers already short-circuit on byte equality
(`src/command/server/receiver/hooks/artifact.rs::needs_rewrite`,
`hooks/json.rs`). `src/sync/args.rs` says so explicitly in a comment: *"Brain's
`.opencode/plugins/brain.js` bridge is not excluded; that is content every
machine needs."*

The mechanism is baseline eviction (step 4) plus the `path2` pull (step 5),
sustained by the mid-run write race (steps 1 to 3).

## Provenance: Brain cannot tell whose change is whose

There is no machine identity anywhere in the codebase. `grep -rn
"machine_id\|device_id\|install_id" src/` returns nothing. The only "who" is
`sync::command::hostname()`, which reads `$HOSTNAME` then `hostname(1)`, and on
macOS that drifts with the network. One laptop has filed conflicts under three
different names:

```
~/brain/.brain/hooks/
  agent_session_start_hook (conflict Mac 2026-08-25).py
  agent_session_start_hook (conflict MacBook-Pro-10 2026-08-31).py
  agent_session_start_hook (conflict Avandar-MacBook-Pro 2026-09-05).py
```

`docs/decisions.md` already concedes the consequence for the task CSVs: *"The
CSV lane has no per-machine author field."*

A durable per-machine UUID is cheap and unlocks both provenance and the fast
"nothing changed" probe in the speed section. Constraints found during review:

- It must live in `~/.config/brain/env.json` as a **structural, non-writable**
  field alongside `root` (`src/env/schema.rs::is_structural`), not a declared
  `VarSpec`, or `brain env set` could rewrite machine identity.
- Cloning a machine (Migration Assistant, Time Machine, `rsync ~/.config`)
  duplicates it, which is strictly worse than hostname drift because nothing
  can detect it. Bind the mint to a hardware fingerprint (`IOPlatformUUID`) or
  re-mint when the fingerprint changes.
- Per the repo's migration contract, `down` must not re-mint. A
  downgrade-then-upgrade cycle that mints a different id would orphan every
  durable reference. Make `down` a no-op, or mint lazily on first read and skip
  the migration entirely.
- Keep the UUID out of the conflict *filename*. `docs/data-model.md` specifies
  a human-readable `name (conflict <host> <date>).ext` so that resolving one is
  a normal file-manager task; a 36-character UUID destroys that, and
  `parse_conflict_name`'s `rsplit_once(' ')` would accept it silently. Put the
  stable id in the journal and the ledger only.

## When the regression started

Three conflicts across roughly 490 runs on 22 to 24 August. Then `d757a2d
feat: migrate agent hooks into workspaces` (2026-08-15) put per-machine
lifecycle artifacts inside the synced root, and `d5a3f47 feat(receiver):
produce lifecycle observations` (2026-08-25) added another.

Conflict copies created per day, against the number of sync runs that day:

| Date | Runs | Copies | |
| --- | --: | --: | --- |
| Aug 22 | 2 | 0 | |
| Aug 23 | 292 | 2 | `▏` |
| Aug 24 | 196 | 1 | `▏` |
| Aug 25 | 185 | 49 | `██` |
| Aug 26 | 229 | 132 | `████` |
| Aug 27 | 97 | 91 | `███` |
| Aug 29 | 97 | 69 | `██` |
| Aug 30 | 158 | 66 | `██` |
| Aug 31 | 431 | 384 | `█████████████` |
| Sep 1 | 22 | 12 | `▍` |
| Sep 2 | 168 | 182 | `██████` |
| Sep 3 | 360 | 597 | `████████████████████` |
| Sep 4 | 35 | 170 | `██████` |
| Sep 5 | 9 | 10 | `▎` |

Note 23 and 24 August: roughly 490 runs, three conflict copies between them.
The engine was behaving.

## Candidate fixes, adjudicated

Three adversarial reviews reproduced each candidate against rclone v1.75.0 on
throwaway path pairs. Five of the original proposals did not survive; two would
have lost data outright.

| Change | Verdict | Why |
| --- | --- | --- |
| **Write barrier**: Brain must not mutate the synced root while that workspace's sync lock is held | **Ship first** | The only change that makes step 1 impossible rather than unlikely. The lock already exists (`src/sync/lock.rs`). |
| **Fix the abort classification**: match rclone's out-of-sync error distinctly from a missing listing | **Ship first** | Every auto-resync Brain has ever run came through this substring collision. Removes the destructive path entirely. |
| **`--resync-mode newer`**, gated strictly inside the `Resync` branch | Ship | Trap: passing `--resync-mode` unconditionally turns *every* run into a resync (measured: rclone starts resyncing with no `--resync` present). Add a test asserting absence for Both/Pull/Push. Also journal "the older side of N files was discarded"; the flag still keeps no copy. |
| **Stop the in-root churn at source**: drop `remove_dir_all` from the skills install; guard `check_access::ensure_local_marker` and `reindex/walk.rs`'s two lookup CSVs with the byte-equality check the hook writer already uses | Ship | 33 files rewritten per launch, and `RCLONE_TEST` re-stamped on every non-Push sync, guaranteeing a path1 delta every run. Prefer one shared idempotent-write helper over a seventh copy of read-and-compare. |
| **Retire `--conflict-resolve path2` from automatic runs**: all five sites | Ship | `periodic.rs`, `tui/runtime/builder.rs` (TUI startup), `tui/app_sync.rs` (receiver freshness), `command/tasks.rs` (non-TUI startup), `workspace/initialize.rs` (emptied-root recovery). Fixing only the timer leaves "stale remote beats my edit" on the startup path, which runs right after a machine was edited offline. |
| **Coalesce triggers** so a settled change is never stranded | Re-scope | Not a speed item; a correctness one. `--if-idle` *drops* the run (`WorkspaceLockOutcome::Coalesced`) and the debouncer disarms on fire, so a settled edit can wait for the next filesystem event or the next periodic run. The C4 decision doc claims the opposite and is wrong. |
| Exclude the "machine-local" artifacts from sync | **Drop** | **Would have lost data.** Five skills exist nowhere but `.agents/skills/` (`email-triage`, `equity-research-supplement`, `sales-qualification-avandar-research`, `start-drivable-chrome`, `zotero-article-summary`); the tree is mixed, with 7 of 17 directories carrying no `.brain-rendered` marker, and `email-triage/config/` holds live agent-authored state. Excluding `RCLONE_TEST` breaks `--check-access` and turns every sync into an abort-plus-resync loop (measured). Excluding `.opencode/plugins/**` and `.claude/settings.json` contradicts live decisions and doc comments. `*.bak` is user content and belongs in the per-user `SyncConfig::exclude` knob, not hardcoded. |
| Stop excluding `*.__brainconflict__*` | **Drop** | Fixes nothing: the baseline eviction reproduces with no excludes at all. Measurably fans markers out to every machine, each renaming them with the wrong host and date, and makes `verify::classify` report a conflict on a machine that had none. Keep `*(conflict *)*` too: it is what keeps friendly copies local-only, which the `brain sync resolve` remote-orphan design depends on. |
| `--conflict-loser delete` | **Drop** | Deletes the loser with no copy anywhere and still reports `Bisync successful`, so Brain would journal `Clean`. Reverses the C2 decision, and removes the very bytes a rollback ledger exists to preserve. Also does not fix the loop: with equal mtimes rclone declares no winner and suffixes *both* sides. |
| Make the watcher push a real bisync | **Drop** | Already rejected by name in the C4 decision. Gives a null-stdio background child delete authority under a `--max-delete` default of **50 percent**; creates a genuine local-write feedback loop; and puts the unattended resync on a path that fires 4 to 5 times a minute. It also falsifies four doc promises of one-way/non-deleting behavior. |
| Set `--modify-window` explicitly | **Drop** | A wider window makes "no winner" more frequent, which manufactures exactly these conflicts, and hides same-size edits whose mtime moved less than the window. B2 stores the source mtime in `src_last_modified_millis`, so remote modtimes are not the problem. |

### Ordering hazard

Do **not** clean up the 44 conflict copies or run `brain sync repair` yet.
`repair` is a bare `--resync` today, so the cleanup would itself be the
data-loss event in step 3. And eight of the 44 copies hold content that differs
from the original; those need review, not a bulk delete.

Safe sequence:

1. Abort-classification fix
2. `--resync-mode newer`, gated to the `Resync` branch
3. Write barrier
4. In-root churn fixes
5. Retire `path2` from all five automatic sites
6. Trigger coalescing
7. Content-verified cleanup of the 44 copies and 17 remote orphan markers

## The one genuinely structural gap

This is worse than the conflict noise and unrelated to it. Two facts compose
badly:

1. The "workspace-scoped" sync lock resolves under
   `~/.cache/brain/workspaces/<uuid>/` (`WorkspacePaths::sync_lock`), so it is
   **machine-local**. It serializes syncs on one laptop, not across machines.
   With a five-minute timer per open shell per machine, concurrent runs are the
   steady state, not a race.
2. The CSV lane is an uncoordinated read-modify-write on one shared remote
   object (`csv_sync/transport.rs`: `batch_download`, merge, `batch_upload`),
   and B2 offers no compare-and-swap.

So machine A can publish a merged CSV that machine B then overwrites from an
older read. On A's next sync the row is in A's baseline, unchanged locally, and
absent remotely, which `csv_merge` reads as "the remote deleted it" and drops
locally. A task disappears, and nothing reports it.

```rust
// src/sync/csv_merge/merge.rs
(Some(base), Some(side), None) | (Some(base), None, Some(side)) => {
    if side == base {
        report.deleted += 1;   // row dropped: "present in my baseline,
    } else {                   // unchanged locally, gone remotely"
        rows.insert(id.to_owned(), side);
    }
}
```

Contrast `src/sync/counters.rs`, which is a monotonic max-join and immune to
the same interleaving. The difference is that one lane infers meaning from
absence.

The fix is in-house and small, and takes one of two shapes:

- A **remote lease**, reusing the append-only lowest-UUID election protocol
  that `sync/identity/claim.rs` already implements for workspace claims. That
  protocol exists precisely because B2 exposes no portable compare-and-swap.
- **Single-writer-per-object state files**
  (`tasks/.state/<machine-uuid>.csv`) merged as a join, with explicit
  tombstones so a deletion is stated rather than inferred.

Write the RED test first: a two-machine interleaved-upload harness asserting
that a row present locally and absent remotely is never dropped without a
tombstone.

## Speed

The workspace is 145 GB but only about 3,880 objects, so object count is not
the constraint. Almost none of the cost is transfer.

**A one-second full stat walk of 145 GB.** On macOS the watcher is
`notify::PollWatcher` with a one-second poll interval (`src/sync/watch.rs`),
recursive over the whole root and unfiltered by the exclude set. This is the
single largest suspect for "slow" and for a hot machine, and no proposal
originally touched it. Raise the interval sharply, apply the exclude set to the
walk, or reinstate FSEvents with the poll watcher as a fallback rather than the
default.

**Self-inflicted trigger storm.** Fifteen detached sync children in five
minutes (13 push, 2 pull). A pull writes under the watched root, so each
writing pull provokes 4 to 5 follow-on pushes; quiet pulls provoke one:

| Minute | Pulls | Pushes | Pull produced conflicts? |
| --- | --- | --- | --- |
| 10:37 | 1 | 4 | yes (5 renames) |
| 10:42 | 1 | 4 | yes |
| 10:47 | 1 | 5 | yes |
| 10:52 | 1 | 1 | no |
| 10:57 | 1 | 1 | no |

Each push then spends its run issuing `Updated modification time in
destination` for the same 33 skill files. The 3-second debounce
(`default_debounce_ms`) is far too tight for a tree Brain itself writes into.

**Nine rclone processes per run.** A single pull spawns nine rclone processes
and took 23.8 s wall clock. Each pays a roughly 0.6 s B2 authorization floor
before doing any work. Already tracked as **BR-6** (an `rclone rcd` daemon); do
it after the correctness fixes, not before.

**A cheap "nothing changed" probe.** The only speculative idea worth keeping. A
per-machine `heads/<uuid>` object holding a tree hash, read in one `rclone
lsjson`, lets an unchanged sync skip both listings: roughly 7 s down to under
1 s. Needs the machine UUID, a local stat-gated hash index, data-then-head
write ordering, per-machine heads rather than a shared counter, and a forced
full reconcile every N runs. Additive, not a rewrite.

**`brain check` may be hashing the whole tree.** `src/sync/check/mod.rs` reuses
the bisync argv and appends `--compare size,checksum`. rclone keeps no local
hash cache, so that plausibly reads 145 GB per invocation. Measure it before
optimizing anything else.

**Free wins from the churn fixes.** Dropping the `remove_dir_all` rewrite
removes 33 uploads plus their listing deltas per launch, and removes the step-1
race trigger at the same time. The 40-plus "can't follow symlink" notices per
run are log noise only: `.claude/skills/**`, `.codex/skills/**`,
`.opencode/skills/**` and root `CLAUDE.md` are symlinks that rclone already
skips, because Brain never passes `-l` or `-L`.

## Current mess to clean up

| Location | Item | Count |
| --- | --- | --- |
| `~/brain` | friendly `(conflict …)` copies | 44 (36 byte-identical, 8 differing) |
| `~/brain` | raw `__brainconflict__` markers | 5 |
| remote bucket | orphan `__brainconflict__` markers | 17 |
| remote bucket | friendly `(conflict …)` copies | 0 |

The two sides do not even agree on the conflict set: friendly copies are a
bisync exclude, so they stay local-only, while the raw markers accumulate on
the remote where no Brain command surfaces them. `docs/decisions.md` C2 admits
this leak. Three of the 17 remote markers are real user content
(`areas/avandar/legal-finance/policies/…docx`,
`projects/personal__build-diff-comment-style-guide/…`,
`resources/email-digests/…`) stranded with no local counterpart.

## Prior art

Grounded in mechanism. Everything in the "copy" column is buildable in-house
against a plain bucket; everything in the "leave" column assumes
infrastructure deliberately refused.

| System | Copy | Leave |
| --- | --- | --- |
| Syncthing | Nearly all of the model: a device ID minted once and never derived from hostname; a local stat-gated hash index; per-file version vectors; device-named conflict files; tombstone retention; index plus periodic full rescan. | The peer-to-peer block protocol. Brain has a bucket, not peers. |
| Unison | The fastcheck fallback: when stat says "maybe changed", compare content before deciding. That is precisely the half Brain lacks. Also: refuse rather than guess. | Interactive per-file prompting, the very UX BR-20 exists to abolish. |
| git | The index stat-cache trick, and objects-before-refs write ordering (the same discipline the head-pointer probe needs). | Whole-blob content addressing for a 145 GB media tree, and GC. Git's own answer to this is LFS. |
| CouchDB | The durable per-peer replication checkpoint, and retain-the-loser rather than discard. | Per-file revision trees and `_changes` semantics; they need a database server. |
| Seafile | Confirmation that git-for-files works when you own the server. | The model on a dumb bucket. Its GC data-loss history is the cost estimate. |
| Dropbox | Nothing structural. Cite it as proof that the clean answer needs a server-side conditional write that B2 does not offer. | Anything assuming that write exists. |
| CRDTs | Brain already built the right scoped version: `csv_merge`'s convergence and idempotency are proved as unit tests. That is the pattern to extend. | CRDTs over opaque files. A PDF has no merge function, which is why BR-20's LWW register plus a rollback ledger is the right shape. |

### On the ledger-as-truth idea

A SQLite oplog that *is* the source of truth is unsound here: the brain root is
edited by Finder, agents, editors, and arbitrary CLI tools, so an oplog Brain
does not author would be a lie. The weaker form is sound, and is what the CSV
lane already does: Brain *derives* the record by observing content, rather than
authoring intent. `csv_sync::baseline_path` is exactly that. Ledger-as-record:
yes. Ledger-as-truth: no.

The same reasoning rejects "pull only the ledger, then fetch what it points
at." That is what bisync's two listings already are, and the listings are not
the bottleneck: a dry-run bisync with `--fast-list` is 6.9 s of a 7.2 s
no-change sync, and 3,880 objects is small. The bottleneck is process
authentication and the local walk.

## Related tasks

- **BR-20** (filed by this investigation): never leave a conflict unresolved;
  force `lww-register` and record it in a rollback ledger. Includes the two
  measured rclone constraints found here (keep `--conflict-loser pathname` and
  delete the loser after recording it; handle the "no winner" state where
  rclone suffixes both sides and the canonical name disappears from both).
- **BR-11**: resolve sync conflicts interactively with an LLM handoff. Ships
  the **opposite default** to BR-20. One of the two must be rescoped before
  either starts.
- **BR-6**: reuse one rclone process per sync instead of re-authenticating per
  call. Ordered after the correctness fixes.

## Findings not yet filed as tasks

1. Write barrier plus abort-classification fix (highest priority; the
   data-loss path).
2. CSV lane lost update and the machine-local sync lock.
3. Retire `--conflict-resolve path2` from all five automatic sites.
4. Trigger coalescing so a settled change is never stranded.
5. `PollWatcher` one-second full-tree stat walk on macOS.
6. Stable per-machine identity.
7. Content-verified cleanup of the local copies and remote orphan markers.

## Tests and docs the fixes will need

No test currently covers:

- A guard that no `EXCLUDES` pattern matches `RCLONE_TEST`.
- `EXCLUDES` agreeing with the frontend registry's lifecycle paths
  (`src/agent/registry/contract.rs`), so the hand-maintained glob array cannot
  silently drift when a frontend is added.
- `--resync-mode` present only for `Direction::Resync`.
- A coalesced trigger is not stranded.
- Remote-orphan reaping after any new exclude.
- The abort classifier distinguishing an out-of-sync listing from a missing
  one.

Docs to update alongside the fixes, per the repo contract: the `EXCLUDES` doc
comment in `src/sync/args.rs`, `docs/decisions.md` C2 and C4,
`docs/features.md` (the one-way/non-deleting watcher promise),
`docs/architecture.md`, `docs/integrations.md`, `docs/data-model.md`, and
`docs/config.md`.

---

*No source files were modified during this investigation. `~/brain` and the
bucket were read-only throughout; the only writes were the two
project-management commits for BR-20.*
