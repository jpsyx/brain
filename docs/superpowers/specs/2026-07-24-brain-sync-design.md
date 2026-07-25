# Brain sync (Sub-project C) — design

- **Date:** 2026-07-24
- **Status:** Design — forks resolved; ready for phase-by-phase implementation planning (C1 first).
- **Scope of this document:** the **full design for Sub-project C**, the last of
  the A → B → C program that makes `brain` generic. A (personalization/config
  foundation) and B (skill pipeline) are shipped. C adds **cross-machine sync of
  the brain contents**, local-first, via **Backblaze B2** — "independent of
  big-tech (no Dropbox)", goal 4 of the
  [generic-foundation spec](2026-07-23-brain-generic-foundation-design.md).

---

## 1. Why

The brain is the user's synced, portable knowledge + task store. Today it lives
on one machine. C makes `~/brain` the same across every machine the user owns:
edits, additions, **and deletions** propagate both ways through a private B2
bucket, with tasks/habits merging cleanly and prose files never silently losing
an edit.

The original program sketch described C as a hand-rolled "watcher + append-only
ledger of every filesystem mutation + B2 backend + reconciliation". During
design we chose a **thinner, safer** architecture: **`rclone bisync` owns the
transport, diffing, and bidirectional reconciliation** (a mature, battle-tested
engine), and brain owns only the parts rclone can't: the `brain sync` UX, the
conflict policy, and a **semantic merge for the two CSVs**. Hand-rolling
bidirectional multi-machine sync — especially correct deletion propagation and
conflict handling — is a large, data-loss-prone surface we deliberately don't
own. See §12 for the decision record.

## 2. Resolved design forks (the settled decisions)

| Fork | Decision |
|---|---|
| Transport | **`rclone bisync`** to a **private B2 bucket**, brain wrapping it. |
| Encryption | **Private bucket + B2 server-side encryption (SSE).** No client-side crypt/key to manage or lose. Upgradeable to `rclone crypt` later without changing the brain surface. |
| Conflicts (prose) | **Keep-both conflict copies** — the loser is preserved as `name (conflict <host> <date>).ext`; nothing is silently lost. |
| Conflicts (CSVs) | **`tasks.csv` / `habits.csv` never conflict** — brain excludes them from bisync and does an **id-keyed 3-way merge**. |
| Conflict resolution UX | Agent-driven: `brain sync conflicts` enumerates copies; a new **`/second-brain resolve-conflicts`** workflow has Claude read the competing copies + timestamps, merge, and delete the extras. |
| Deletions | **Propagated both ways** by bisync, guarded by `--max-delete`, and **verified post-sync**; anything rclone can't reconcile is journalled and surfaced. |
| Triggers | **Manual `brain sync`** + **auto on brain start/exit** + a **continuous watcher that is on by default whenever sync is configured**. Plus `/second-brain sync` (shells out to the CLI). |
| Ledger | Narrows to a **sync journal + CSV-merge baseline** in the machine-local cache (rclone owns file diffing). |

## 3. The config split C forces (machine-level vs brain-internal)

C introduces machine-level state (B2 credentials, bucket) that **must not** ride
the brain-dir sync. This finally splits brain's config into two stores by
**lifecycle**, cleanly separating what syncs *with the brain* from what is
*per-machine*.

| Store | Path | Holds | Synced how |
|---|---|---|---|
| **Machine-level config** | `~/.config/brain/config.json` (fixed XDG path, **outside** the brain root) | `root`, the `sync` block (B2 creds, bucket, trigger flags), `claude_cmd`, `markdown_to_pdf_path`, `calendar_id`, `agenda_dir`, `skills_auto_sync`, … | **Not** by C. The user *may* privately track it in `jpsyx-configs` at `home/.config/brain/` (private repo). brain never knows about jpsyx-configs. |
| **Brain-internal config** | `<brain-root>/.config/` | `personalization.json`, `extensions/<skill>.md`, `plugins/<name>/` | **By C**, riding the brain-dir sync — your personalization/extensions travel with your brain. |

- **`root` becomes a key** in `~/.config/brain/config.json`, replacing the
  `~/.config/brain-root` one-line pointer. Because the config file now lives at a
  fixed path *outside* the brain root, the old circular-dependency argument ("you
  need the root to find the setting that names the root") no longer applies:
  `~/.config/brain/config.json` is found without knowing the root. The legacy
  `~/.config/brain-root` pointer is still read for back-compat and migrated into
  the `root` key on first run.
- **This partially reverses the A-era decision** (decisions.md: "unify everything
  inside the brain root") that was made to dodge the jpsyx mirror-write footgun.
  The reversal is correct because C makes the *opposite* problem dominant:
  machine-level secrets/paths cannot be allowed to sync via the brain dir. See
  §12 for the footgun's residual handling.

## 4. Architecture

```
brain sync
  ├─ preflight: rclone present? sync configured? (else guide setup)
  ├─ Lane A — everything except the CSVs
  │     rclone bisync  <brain-root>  b2:<bucket>/<path>
  │       · propagates creates / edits / DELETES both ways
  │       · keep-both conflicts → "name (conflict <host> <date>).ext"
  │       · --max-delete guard; --filter excludes the two CSVs
  ├─ Lane B — tasks.csv / habits.csv (excluded from Lane A)
  │     fetch remote copy → id-keyed 3-way merge(base, local, remote)
  │       → write merged locally → push merged remote → update baseline
  └─ postflight: verify expected == actual; journal runs, conflicts,
                 and anything rclone did not reconcile
```

- **Transport.** `rclone bisync` between `<brain-root>` and `b2:<bucket>/<path>`.
  First run on a machine is `rclone bisync … --resync` to establish the baseline
  (or to bootstrap a fresh machine by pulling the whole brain down). rclone's B2
  remote is constructed **on the fly from the config `sync` block** (connection
  string / `RCLONE_CONFIG_*` env), so brain needs **no persisted `rclone.conf`**
  and all Backblaze config stays in `~/.config/brain/config.json`.
- **Lane A (prose & everything else).** Straight bisync. Conflicts resolve
  keep-both: newer becomes canonical, the loser is renamed to the friendly
  `name (conflict <host> <date>).ext` (via rclone conflict flags + a brain
  post-pass that maps rclone's suffix to the friendly name). Deletions on either
  side propagate to the other.
- **Lane B (the two CSVs).** Excluded from bisync by a generated `--filter`.
  brain fetches the remote CSV to a temp path and runs a **pure id-keyed 3-way
  merge** against the last-synced **baseline** (cached locally) and the local
  file, then writes the merged result locally and pushes it back. Union by row
  id; additions, completions, and deletions from both machines merge without a
  conflict copy (§6).
- **The journal (narrowed "ledger").** A small SQLite store in the machine-local
  cache (`~/.cache/brain/sync/`, rebuildable, **not** synced) recording each sync
  run, its outcome, conflicts produced, and the CSV baselines. rclone owns file
  diffing, so brain does **not** re-log every filesystem mutation.
- **Post-sync verification.** After each run brain compares the expected result
  against the actual local+remote state and **journals anything rclone did not
  reconcile** (a bisync abort, a `--max-delete` trip, a filtered file). This is
  the "whatever rclone can't sync, we track and handle separately" contract.

## 5. The `brain sync` command surface

| Invocation | Effect |
|---|---|
| `brain sync` | Push + pull now (Lane A bisync + Lane B CSV merge + verify). |
| `brain sync --pull` / `--push` | One direction only (still uses bisync semantics; `--push`/`--pull` bias the conflict/precedence handling). |
| `brain sync setup` | Interactive: check rclone, capture B2 keyId/appKey/bucket, write the `sync` block, create/verify the bucket, run the initial `--resync`. |
| `brain sync init` / `brain sync --resync` | (Re)establish the bisync baseline — first run, or recovery after a "prior listing missing" abort. Bootstraps a fresh machine. |
| `brain sync status` | Last run time/result, pending local changes, open conflicts, watcher state. |
| `brain sync conflicts` | List conflict copies with their host/timestamps (the enumerator the skill's resolve flow consumes). |

- Like `brain config`, `brain sync` is exempt from the `markdown-to-pdf` startup
  gate so you can always fix/inspect your sync.
- Sync is **opt-in**: absent a configured `sync` block, brain behaves exactly as
  today and all sync paths no-op with a one-line hint.

## 6. Conflict model

- **Prose / resources / `.config/`:** keep-both. The winner keeps the canonical
  name; the loser is `name (conflict <host> <date>).ext` beside it. `brain sync`
  reports the count; the files sit in place until resolved.
- **`tasks.csv` / `habits.csv`:** id-keyed 3-way merge, so the files edited most
  never spawn conflict copies:
  - Row in one side only vs. baseline → **added** → keep.
  - Row in baseline, absent one side → **deleted** → delete (honors §2 deletions).
  - Row changed on both sides → field-level union; **completion wins** over open;
    for a genuine same-field divergence, resolve by a per-row `modified`
    timestamp if present (see below), else keep local and journal it as a soft
    conflict for review (rare).
  - **Proposed schema addition:** a `modified` timestamp column on the task/habit
    row makes same-field merges **deterministic** (per-row last-writer-wins).
    Decided in C3; if we don't add it, the keep-local-and-journal fallback holds.
    This is a `data-model.md` change and must not break `mark_done.py` or the
    existing CSV readers.
- **Resolution (agent-driven, in second-brain).** `/second-brain resolve-conflicts`
  walks the `brain sync conflicts` list, has Claude read each set of competing
  copies + timestamps, merge them into the canonical file, and delete the extras
  — the manual-but-assisted flow the user asked for. Generic (no personal data).

## 7. Triggers

- **Manual:** `brain sync` any time.
- **Auto on start/exit:** the persistent shell pulls on launch and pushes on exit
  (and on an idle timer), when sync is configured. `on_start` / `on_exit` config
  flags, default on once sync is enabled.
- **Continuous watcher:** a debounced filesystem watcher (`notify` crate) syncs
  changes live. **On by default whenever the `sync` block is configured** (bucket
  present); set `sync.watch=false` to disable. The watcher only triggers a
  debounced `brain sync`; it does not maintain its own mutation ledger.
- **`/second-brain sync`:** a skill row that shells out to `brain sync`, so the
  user can sync from inside a Claude session.

## 8. second-brain skill integration (generic, core skill)

Two additions to the bundled `second-brain` core skill (must stay 100% generic —
the `bundled_skills_carry_no_personal_data` guard applies):

- **`sync`** — run `brain sync` and report the summary.
- **`resolve-conflicts`** — the agent-driven conflict resolver of §6.

Both are described generically; no bucket names, hosts, or personal paths in the
skill text.

## 9. Config schema (`~/.config/brain/config.json`)

```jsonc
{
  "root": "~/brain",
  "claude_cmd": "…",
  "markdown_to_pdf_path": "…",
  "calendar_id": "…",
  "agenda_dir": "~/Downloads",
  "skills_auto_sync": true,

  "sync": {
    "enabled": true,
    "b2_bucket": "my-brain-bucket",
    "b2_path": "",                 // optional prefix within the bucket
    "b2_key_id": "…",              // B2 application keyId
    "b2_app_key": "…",             // B2 application key (secret; machine-local)
    "on_start": true,
    "on_exit": true,
    "watch": true,                 // default true when sync is configured
    "max_delete_percent": 50       // bisync safety guard
  }
}
```

- All `sync` fields optional; a missing `sync` block ⇒ sync disabled, brain
  unchanged. Unknown top-level keys ignored (forward-compat).
- `root` accepts `~`-expansion; back-compat falls back to `~/.config/brain-root`
  then `~/brain`.

## 10. Prerequisites & doctor

- **rclone** becomes a prerequisite alongside `markdown-to-pdf`. `brain tasks
  doctor` and `brain sync setup` check for it and print install guidance; sync
  paths degrade gracefully (no-op + hint) when it's absent.
- No new *Rust* dependency for the transport (rclone is an external binary,
  invoked via a thin `Command` shell). The **only new crate is `notify`** for the
  watcher (justify in `docs/architecture.md`).

## 11. Security / privacy

- **Private bucket + SSE.** Backblaze holds the keys (not zero-knowledge) — an
  accepted tradeoff for simplest recovery. The design leaves a clean seam to
  layer `rclone crypt` later without changing `brain sync`.
- **Secrets are machine-local.** B2 keys live only in `~/.config/brain/config.json`
  (outside the synced brain dir), so they never land in the B2 bucket. If the
  user tracks that file in the **private** `jpsyx-configs` repo, the secret sits
  in a private repo by their explicit choice; brain itself is agnostic.
- **`--max-delete` guard** prevents a corrupted/empty local state from mass-
  deleting the remote brain (and vice-versa); a trip aborts and journals for
  review rather than propagating.

## 12. Cross-cutting invariants (inherited from the program) + decisions

- **Repo stays 100% public and generic.** No bucket names, hosts, keys, or
  personal paths committed. Personal sync setup lives only in the user's
  machine-local config (§14).
- **brain never writes or knows `jpsyx-configs`.** It reads/writes a standard XDG
  file; whether the user syncs that file privately is outside brain's concern.
- **Decision — rclone over a hand-rolled engine.** Bidirectional deletes +
  conflict handling are the hard, dangerous core of sync; rclone bisync is mature
  and correct there. brain keeps only the semantic pieces (CSV merge, conflict
  UX, journal/verify). Recorded in `docs/decisions.md`.
- **Decision — the residual jpsyx mirror-write footgun.** `~/.config/brain/config.json`
  is runtime-mutable (`brain config set`, `markdown_to_pdf_path` self-heal). If
  the user mirror-symlinks it via jpsyx (writes land in the wiped `home-dist/`
  mirror and are lost), the fix is a **jpsyx-side** concern: seed/copy the file
  rather than symlink it, or re-commit after changes. brain stays generic and
  does nothing special. Flagged here so the user handles it in their jpsyx setup;
  it does not change brain's design.

## 13. Phase decomposition (each its own spec → plan → RED/GREEN build → docs)

- **C1 — Config relocation & lifecycle split.** Move machine-level config to
  `~/.config/brain/config.json`; `root` becomes a key (deprecate the pointer,
  read-back-compat + one-time migration); add the `sync` block **schema** (parse
  only, no behavior). Establish the invariant that machine-level config is not
  Backblaze-synced and `~/brain/.config/` is. Pure config parse/migration tests.
- **C2 — Sync core (manual).** rclone bisync transport (private bucket, SSE, creds
  via on-the-fly remote); `brain sync [setup|init|status|--push|--pull]` + bare
  `brain sync`; keep-both conflicts; **bidirectional deletions** with `--max-delete`;
  the sync journal + **post-sync verification**; rclone doctor/prereq check. CSVs
  ride Lane A for now (fallback keep-both).
- **C3 — CSV semantic merge.** Exclude the CSVs from bisync; the pure id-keyed
  3-way merge; optional `modified` column for deterministic per-row LWW;
  conflict-free task/habit sync.
- **C4 — Triggers.** Sync on brain start/exit; the `notify` watcher, **default-on
  when sync is configured**; TUI lifecycle hooks + debounce.
- **C5 — Skill integration + migration.** `/second-brain sync` and
  `/second-brain resolve-conflicts` (generic); `brain sync conflicts` enumerator;
  the user's migration (§14).

No cut: the watcher (C4) ships as a first-class trigger, not deferred.

## 14. Migration (the primary user, no regression)

- Relocate this machine's config from `<brain-root>/.config/config.json` +
  `~/.config/brain-root` into `~/.config/brain/config.json` with `root` as a key
  (one-time, automatic, back-compat read).
- Create the B2 bucket; run `brain sync setup` on each machine to capture that
  machine's B2 key + bucket; enable sync.
- Track `~/.config/brain/config.json` privately in `jpsyx-configs` at
  `home/.config/brain/` (handling the §12 footgun on the jpsyx side).
- Verify: an edit/add/delete on machine A appears on machine B after a sync;
  tasks/habits merge with no conflict copy; a concurrent prose edit yields exactly
  one conflict copy and resolves via `/second-brain resolve-conflicts`.

## 15. Testing (pure-function first, per house rules)

- **CSV 3-way merge** (the crown jewel): add/complete/delete on each side;
  same-id divergent edits; deletion vs edit; `modified`-based tiebreak; idempotent
  re-merge.
- Config parse/migration: pointer→`root`-key migration; `sync` block defaults;
  missing block ⇒ disabled; secret round-trips but never enters the sync set.
- Conflict-copy **name builder** (`name (conflict <host> <date>).ext`) and the
  rclone-suffix→friendly-name mapper.
- The bisync **filter/flag builder** (CSV exclusion, `--max-delete`, conflict
  flags) as a pure function; no live B2 in unit tests.
- **Post-sync verification** classifier (expected vs actual; what counts as
  "unreconciled").
- Watcher **debounce** classifier (pure); the `notify` shell stays thin.
- rclone invocation is shelled; test the **argument builder**, not the network.

## 16. Docs to update (same change, per phase)

- `docs/config.md` — the two-store lifecycle split; `~/.config/brain/config.json`;
  `root` as a key; the `sync` block; rclone prerequisite.
- `docs/features.md` — `brain sync` + subcommands; auto/watcher triggers;
  `/second-brain sync` + `resolve-conflicts`; conflict-copy behavior.
- `docs/architecture.md` — the sync module + rclone shell; `notify` dependency;
  Lane A/B data flow; journal in the cache.
- `docs/data-model.md` — the `sync` config block; the optional CSV `modified`
  column; the journal/baseline schema.
- `docs/integrations.md` — rclone/B2 integration; how the remote is built from
  config; `--max-delete`; setup flow.
- `docs/decisions.md` — rclone-over-hand-rolled; SSE-over-crypt; the config-split
  reversal + residual footgun; watcher-default-on-when-configured.
- `docs/keybindings.md` + palette/menu rows — only if C adds a keybinding or
  palette/menu row for sync.
- The docs-contract table in `AGENTS.md`/`CLAUDE.md` — add sync rows.

## 17. Acceptance criteria (program-level; each phase carries its slice)

1. `~/.config/brain/config.json` is the machine-level store; `root` is a key;
   legacy pointer migrates; secrets never enter the B2 bucket.
2. `brain sync` propagates creates, edits, **and deletes** both ways through a
   private B2 bucket; `--max-delete` guards catastrophes; post-sync verification
   journals anything unreconciled.
3. `tasks.csv` / `habits.csv` merge by id with no conflict copy for
   add/complete/delete; prose conflicts produce exactly one keep-both copy.
4. Sync runs manually, on brain start/exit, and via the watcher (default-on when
   configured); `/second-brain sync` and `/second-brain resolve-conflicts` work.
5. The repo stays generic (no bucket/host/key/personal path committed); the
   binary carries no personal data.
6. The primary user's machines sync with no regression to existing behavior.
7. `cargo test --release` green; `cargo clippy --release --all-targets` clean.

## 18. Open questions (resolved per-phase, not now)

- Exact rclone conflict flags vs. a brain post-pass to produce the friendly
  conflict-copy name (C2).
- Whether to add the CSV `modified` column now or keep the journal-fallback (C3).
- Watcher debounce window + idle-push interval defaults (C4).
- `brain sync setup` UX: pure prompts vs. delegating parts to `rclone config` (C2).
- Whether a future `rclone crypt` upgrade is worth a config seam now (C2, likely
  "leave the seam, don't build it").
