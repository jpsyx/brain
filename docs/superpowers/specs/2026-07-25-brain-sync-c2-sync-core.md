# Brain sync C2 — Sync core (manual) — design

- **Date:** 2026-07-25
- **Status:** Design — refinement of the approved parent design; ready for implementation planning once C1 lands.
- **Scope of this document:** the **full design for phase C2** of Sub-project C.
  It refines the transport/command/journal design sketched in the
  [parent brain-sync spec](2026-07-24-brain-sync-design.md) (§4–§11) into an
  implementable shape. C2 delivers **manual cross-machine sync**: `brain sync`
  over `rclone bisync` to a private Backblaze B2 bucket, with keep-both
  conflicts, bidirectional deletes, a sync journal, post-sync verification, and
  the `brain sync setup` flow. **Builds on C1** (brain env + the parse-only
  `SyncConfig`).

---

## 1. What C2 delivers

After C2, on a machine with sync configured (`brain sync setup` done):

- `brain sync` pushes + pulls the brain directory to/from a private B2 bucket,
  propagating creates, edits, **and deletes** both ways.
- Concurrent edits to the same prose file yield **keep-both** conflict copies
  (`name (conflict <host> <date>).ext`); nothing is silently lost.
- A `--max-delete` guard prevents a corrupted local state from mass-deleting the
  remote (and vice-versa).
- Every run is journalled; anything rclone couldn't reconcile is surfaced.
- Sync is **opt-in and non-destructive to today's behavior**: with no `sync`
  block configured, `brain` behaves exactly as it does after C1.

Not in C2 (later phases): the id-keyed CSV semantic merge (C3 — in C2 the CSVs
ride the normal file lane with keep-both as the fallback); auto/watcher triggers
(C4 — C2 is the manual command only); the `/second-brain` skill rows (C5).

## 2. Prerequisite: rclone

- **rclone** is the transport. It is **not** a hard startup gate (unlike
  `markdown-to-pdf`): `brain` starts and runs fine without it. Only the `brain
  sync` paths need it, and they degrade gracefully — a missing rclone prints a
  one-line install hint and exits non-fatally.
- `brain tasks doctor` gains an rclone line (present/version, and whether sync is
  configured), so the health-check surfaces setup state.
- No new Rust crate: rclone is an external binary invoked through a thin
  `Command` shell. `rusqlite` (already a dependency) backs the journal.

## 3. Module layout (`src/sync/`)

C1 created `src/sync/{mod.rs, config.rs}`. C2 fills the module out, one
responsibility per file (house rule), keeping the pure builders separate from
the `Command`/FS shells:

| File | Responsibility | Pure? |
|---|---|---|
| `config.rs` *(C1)* | `SyncConfig` (the env `sync` block) | pure |
| `remote.rs` | Build the rclone B2 remote from `SyncConfig`: the `RCLONE_CONFIG_*` **env vars** + the `<remote>:<bucket>/<path>` argument. Secrets go via env, never argv. | pure |
| `args.rs` | Build the `rclone bisync` argument vector + the filter/exclude rules + conflict flags + `--max-delete`, from `SyncConfig` + a direction. | pure |
| `run.rs` | Invoke rclone (`Command`), capture stdout/stderr/exit, parse the summary (transfers/deletes/errors) into a typed `RunOutcome`. | thin IO |
| `conflicts.rs` | The conflict-copy name builder (`stem (conflict <host> <date>).ext`), the rclone-suffix→friendly-name post-pass, and the on-disk conflict enumerator. | pure builder + thin IO |
| `verify.rs` | Post-sync verification: classify a `RunOutcome` (+ a residual-conflict rescan) into reconciled vs. needs-attention. | pure classifier + thin IO |
| `journal.rs` | The SQLite sync journal at `~/.cache/brain/sync/journal.db` (`sync_runs` table): record a run, read recent runs for `status`. | thin IO + pure row map |
| `setup.rs` | `brain sync setup`: check rclone, prompt/confirm bucket + B2 creds, write them into the env `sync` block, verify/create the bucket, run the initial `--resync`. | thin IO |
| `command.rs` | `brain sync` dispatch (bare / `--push` / `--pull` / `setup` / `init` / `status` / `conflicts`), wiring the above. | thin |
| `mod.rs` | Glue + re-exports (`pub use command::run` for `main.rs`). | — |

`main.rs` gains `mod sync;` (C1 deliberately kept it lib-only), a `Sync(SyncArgs)`
clap command, and a dispatch arm **before** the `markdown-to-pdf` gate (you must
be able to sync even when that prerequisite is unset, exactly like `config`/`env`).

## 4. The `brain sync` command surface

| Invocation | Effect |
|---|---|
| `brain sync` | Bidirectional sync now: bisync (push+pull) → conflict post-pass → verify → journal. |
| `brain sync --push` / `--pull` | Bias precedence to the local / remote side for this run (bisync with the corresponding `--conflict-resolve`). Still a bisync, not a one-way mirror, so deletes and new files on the other side are still reconciled. |
| `brain sync setup` | Interactive first-time setup (see §9). |
| `brain sync init` (alias `--resync`) | (Re)establish the bisync baseline: first-ever run on a machine, bootstrapping a fresh machine (pull the whole brain down), or recovery after a "prior listing missing" abort. |
| `brain sync status` | Last run (time, direction, outcome, counts), pending local changes vs. the last baseline, open conflict copies, and whether sync is configured. |
| `brain sync conflicts` | List current conflict copies with host + timestamp (the enumerator C5's `/second-brain resolve-conflicts` consumes). |

All `brain sync` paths run before the `markdown-to-pdf` gate and print to stdout
(diagnostics to stderr), consistent with the binary's stdout contract.

## 5. Transport: rclone bisync

- **Remote, from env, no `rclone.conf`.** `remote.rs` builds the B2 remote purely
  from the `SyncConfig` credentials as **environment variables**
  (`RCLONE_CONFIG_BRAIN_TYPE=b2`, `RCLONE_CONFIG_BRAIN_ACCOUNT=<keyId>`,
  `RCLONE_CONFIG_BRAIN_KEY=<appKey>`) passed to the child process, and references
  the remote on argv as `BRAIN:<bucket>/<path>`. **Credentials never appear on
  argv** (so they don't leak via `ps`), and brain needs no persisted rclone
  config file — all sync config stays in `~/.config/brain/env.json`.
- **The two endpoints.** Local = `brain_root()`. Remote = `BRAIN:<bucket>/<b2_path>`.
- **First run / baseline.** `rclone bisync <local> <remote> --resync` establishes
  the baseline listings (in rclone's own workdir, `~/.cache/rclone/bisync/`,
  machine-local). Subsequent runs are plain `rclone bisync <local> <remote>`.
- **Filters (what syncs).** The brain root syncs *including* its `.config/`
  (brain config, personalization, extensions, plugins — all portable) and the
  task CSVs. A default exclude filter drops machine/VCS cruft: `.git/`,
  `.DS_Store`, `.cache/`, and any `*(conflict *)*` copies (so conflict copies
  themselves don't fan out — they're resolved locally). Brain env (`env.json`)
  is **not** under the brain root, so it is structurally excluded — secrets can
  never reach the bucket.
- `--check-access` (a marker file at the root on both sides) is enabled so a
  half-configured or empty endpoint aborts rather than mirroring emptiness.

## 6. Conflicts (keep-both)

- **Winner + kept loser.** bisync runs with `--conflict-resolve newer
  --conflict-loser pathname --conflict-suffix <MARKER>`, so the newer edit keeps
  the canonical name and the older is preserved with a fixed marker suffix
  (brain does not rely on rclone's date templating).
- **Friendly rename (post-pass).** After bisync, `conflicts.rs` rewrites each
  `<stem><ext><MARKER>` file to `<stem> (conflict <host> <date>)<ext>` — moving
  the marker ahead of the extension, inserting the local `<host>` (`hostname`)
  and the current `<date>` (`YYYY-MM-DD`). The name builder is pure and tested;
  the rename is the thin FS shell. `--push`/`--pull` bias which side wins but the
  loser is still kept, never deleted.
- **Enumeration.** `brain sync conflicts` walks the root for `*(conflict *)*`
  files and reports them with their host/date parsed back out — the list C5's
  resolver consumes. Conflict copies are excluded from sync (§5) so they stay
  local until resolved.

## 7. Deletions + safety

- **Bidirectional.** bisync propagates deletes both ways: a file removed on
  machine A is removed on B at the next sync. This is the core requirement from
  the parent spec.
- **`--max-delete` guard.** Passed from `sync.max_delete_percent` (default 50).
  If a run would delete more than that share of files on either side, bisync
  aborts *without* propagating, and brain journals the abort and prints:
  *"sync aborted: would delete >N% of files. If intentional, run `brain sync
  --resync`."* This prevents a corrupted/empty endpoint from wiping the brain.
- **Never silently destructive.** An abort leaves both sides untouched; recovery
  is an explicit `brain sync init`/`--resync`.

## 8. Post-sync verification + the journal

- **Verification.** `verify.rs` classifies each run from the `RunOutcome`
  (rclone exit code + parsed transfer/delete/error counts) plus a residual scan:
  a run is **clean** only if the exit code is 0, rclone reported no errors, and no
  un-renamed `<MARKER>` conflict files remain. Anything else is **needs-attention**
  and is journalled with the reason (bisync abort, `--max-delete` trip, transfer
  errors, leftover marker files). This is the parent spec's "whatever rclone
  can't reconcile, track and handle separately" contract.
- **The journal.** `~/.cache/brain/sync/journal.db` (machine-local cache, not
  synced), table `sync_runs`:

  | column | meaning |
  |---|---|
  | `id` | autoincrement |
  | `started_at` / `finished_at` | ISO timestamps (passed in; brain avoids ambient clock reads in pure code) |
  | `direction` | `both` / `push` / `pull` / `resync` |
  | `outcome` | `clean` / `needs_attention` / `aborted` |
  | `transferred` / `deleted` / `conflicts` / `errors` | counts parsed from rclone |
  | `note` | abort reason / error summary, else empty |

  `brain sync status` reads the most recent row(s). The CSV-merge **baselines**
  (C3) will live beside this DB under `~/.cache/brain/sync/`.

## 9. `brain sync setup`

Interactive, on `/dev/tty`, idempotent:

1. **Check rclone.** Absent → print the install hint and stop.
2. **Collect config.** Prompt for the B2 bucket, key ID, and application key
   (Enter keeps an existing value). Write them into the env `sync` block via the
   env store (`enabled=true`, `b2_bucket`, `b2_key_id`, `b2_app_key`, `b2_path`),
   so all sync config lives in `~/.config/brain/env.json`.
3. **Verify/create the bucket.** Probe the bucket with the new creds; offer to
   create it if absent (`rclone mkdir BRAIN:<bucket>`). Confirm it is **private**
   (B2 private bucket + server-side encryption is the chosen posture).
4. **Baseline.** Run the initial `brain sync init` (`--resync`) so the machine is
   immediately ready.

Re-runnable any time to rotate keys or point at a new bucket. Like `brain env`,
it runs before the prerequisite gate.

## 10. Security / privacy

- **Secrets are machine-local and off-argv.** B2 keys live only in `env.json`
  (never under the synced brain root) and are handed to rclone via **environment
  variables**, so they neither sync to the bucket nor appear in `ps` output.
- **Private bucket + SSE.** The chosen posture (parent spec §11): Backblaze holds
  the keys. A clean seam is left for a future `rclone crypt` (zero-knowledge)
  upgrade — C2 does **not** build it, and `SyncConfig` needs no new field for it
  yet.
- **`--max-delete` + `--check-access`** are the blast-radius guards.

## 11. Recovery

- **First run / fresh machine.** `brain sync init` (`--resync`) — on a new
  machine with an empty/absent `~/brain`, this pulls the whole brain down.
- **"Prior listing missing" abort.** If bisync aborts because its baseline
  listings are gone (cache cleared, first run without init), brain detects that
  specific failure and instructs `brain sync init`, rather than surfacing rclone's
  raw error.
- **`--max-delete` abort.** Surfaced per §7 with the explicit resync escape hatch.
- brain never auto-resyncs silently (a resync can mask real divergence); recovery
  is always an explicit, one-line-guided user action.

## 12. Testing (pure-first, per house rules)

- **`remote.rs`:** the env-var set + remote arg built from a `SyncConfig`
  (secrets in env, not in the arg); empty/partial config handled.
- **`args.rs`:** the bisync argument vector + filters for `both`/`push`/`pull`/
  `resync`; `--max-delete` value; conflict flags; the default exclude set.
- **`conflicts.rs`:** the friendly-name builder (`stem (conflict host date).ext`
  across weird stems/extensions/no-extension) and the marker→friendly rewrite;
  parsing host/date back out for enumeration.
- **`verify.rs`:** the clean vs. needs-attention classifier over representative
  `RunOutcome`s (clean, transfer errors, abort, leftover markers).
- **`journal.rs`:** row round-trip (insert → read back) against a temp DB;
  pure row mapping.
- **`run.rs` output parsing:** parse representative rclone summary text into
  `RunOutcome` counts (fixture strings; no live rclone).
- **No live B2 in unit tests.** The rclone invocation is shelled; we test the
  **argument/env builders and output parsing**, not the network. An optional,
  explicitly-gated integration test can bisync two local dirs via rclone's local
  backend if rclone is present, but it is not part of the default suite.

## 13. Docs to update (same change)

- `docs/features.md` — the `brain sync` command + subcommands; keep-both conflict
  behavior; the doctor rclone line.
- `docs/integrations.md` — rclone/B2 integration: how the remote is built from env
  (env-var creds, no rclone.conf), `--max-delete`/`--check-access`, the setup flow,
  and the journal at `~/.cache/brain/sync/`.
- `docs/architecture.md` — the `src/sync/` module map + data flow (build → run →
  post-pass → verify → journal); the `Sync` command routing; rclone as an
  external dependency.
- `docs/data-model.md` — the `sync_runs` journal schema; the conflict-copy naming
  convention.
- `docs/config.md` — `brain sync setup` writes the env `sync` block; rclone
  prerequisite (soft, not a startup gate).
- `docs/decisions.md` — secrets-via-env-not-argv; env-var remote over a persisted
  rclone.conf; `--max-delete`/`--check-access` as blast-radius guards; no silent
  auto-resync.
- `docs/keybindings.md` — only if C2 adds a keybinding (it does not; `brain sync`
  is a CLI command, no TUI surface in this phase).
- The docs-contract table in `AGENTS.md`/`CLAUDE.md` — add `brain sync` rows
  (command, transport, journal) pointing at `src/sync/`.

## 14. Acceptance criteria

1. On a configured machine, `brain sync` propagates creates, edits, **and
   deletes** both ways through a private B2 bucket; a fresh machine bootstraps via
   `brain sync init`.
2. Concurrent same-file prose edits produce exactly one `name (conflict <host>
   <date>).ext` copy; `brain sync conflicts` lists it; the winner keeps the
   canonical name.
3. `--max-delete` aborts a catastrophic run without propagating; the abort is
   journalled and surfaced with the resync escape hatch.
4. B2 credentials appear only in `env.json`, are passed to rclone via env vars
   (not argv), and never reach the bucket.
5. Every run is journalled; `brain sync status` reports the last run + open
   conflicts; needs-attention runs are flagged.
6. With no `sync` block configured, `brain` is unchanged from C1 and `brain sync`
   prints a "not configured — run `brain sync setup`" hint.
7. `cargo test --release` green; `cargo clippy --release --all-targets` clean.

## 15. Phase decomposition (for the C2 plan)

Each is a self-contained TDD slice; the plan expands them:

- **C2.1 — Pure builders.** `remote.rs` (env + remote arg) and `args.rs` (bisync
  args/filters/flags). No IO; fully unit-tested.
- **C2.2 — Run + parse.** `run.rs` invokes rclone and parses the summary into
  `RunOutcome`; output-parsing tested against fixtures.
- **C2.3 — Conflicts + verify.** `conflicts.rs` name builder/rewrite/enumerator
  and `verify.rs` classifier.
- **C2.4 — Journal.** `journal.rs` schema + record/read against a temp DB.
- **C2.5 — Command + wiring.** `command.rs`, the `Sync` clap surface, `mod sync;`
  in `main.rs`, dispatch before the gate, `brain sync {,--push,--pull,status,
  conflicts}`; the doctor rclone line.
- **C2.6 — Setup + recovery.** `setup.rs`, `brain sync init/--resync`, the
  "prior listing missing" and `--max-delete` recovery messages.
- **C2.7 — Docs.** Per §13.

## 16. Open questions (resolved in the plan / at kickoff)

- Exact rclone version baseline and the precise `bisync` flag spellings (verify
  against the installed rclone at C2.1; the design is flag-shape-stable).
- The `RunOutcome` summary parser's robustness across rclone output format
  changes — parse defensively (counts default to "unknown → needs-attention").
- Whether `brain sync status` should also show a dry-run diff (`rclone bisync
  --dry-run`) of pending changes, or just the last-run journal (lean: journal +
  a cheap local-change check first; dry-run diff optional later).
- `hostname` source for conflict names (env `HOSTNAME` vs `hostname(2)`); pick a
  portable helper at C2.3.
