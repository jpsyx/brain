# Brain sync — progress, resume, and selective sync — design

- **Date:** 2026-07-25
- **Status:** Design — enhancement to C2 (already merged). Ready for TDD build.
- **Scope:** three improvements to the existing `brain sync`: (1) **live progress**
  during a sync (especially the long first baseline), (2) **seamless resume** after
  an interruption with a hard **never-miss-a-file** guarantee, and (3) **selective
  sync** so large media/data don't have to ride along. Prompted by a real first
  sync: `~/brain` is 144 GB / 6,674 files, but ~143 GB of that is course videos
  (`resources/`) and multi-GB test CSVs (`areas/.../test-data/`) — the actual notes
  are a few hundred KB.

---

## 1. The problem

`brain sync` today spawns rclone via `Command::output()`, which **buffers all output
and blocks until exit**. So a multi-hour first `--resync` shows only
"Establishing the baseline…" with no sign of life — indistinguishable from a hang —
and there's no way to gauge how far along it is. Separately, a huge brain root means
the first sync is long and interruption-prone, and there is no ergonomic story for
resuming, nor a way to keep giant non-note files out of the cloud.

## 2. Live progress (stream, don't capture)

- Replace the capturing `run_rclone` on the interactive path with a **streaming**
  runner: spawn rclone with piped stderr (rclone writes logs/stats to stderr),
  read it **line-by-line on a thread that both echoes to the terminal and appends
  to a capture buffer**, inherit stdout, then parse the captured buffer for the
  journal after exit. Result: the user sees rclone live **and** we still get a
  `RunOutcome` to journal.
- Add periodic progress to the bisync args: `--stats <interval>` with
  `--stats-one-line` (a clean one-liner every N seconds:
  `Transferred: 12.3G / 144G, 9%, 5.2 MByte/s, ETA 6h`). Validate the exact
  flag combo against the installed rclone (v1.74.2) at build time; `--progress`
  is an alternative but its ANSI cursor control tees poorly, so prefer
  `--stats`/`--stats-one-line`.
- `brain sync status` is unaffected (reads the journal); this is purely the live
  run display.

## 3. Resume + the never-miss guarantee

**Invariant (hard):** for every file *in scope* (not excluded, under any size cap),
brain must guarantee it is eventually synced. Overwriting/re-uploading is fine and
expected; **a file silently left un-synced must never happen.** rclone's transfer
model already gives most of this — a re-run skips files whose size/mod-time match
and copies anything missing, so it never re-uploads matched data and never omits a
missing file — but brain must make interruption recovery seamless and refuse to
declare success early:

- Add `--resilient` (and `--recover`, validated) to bisync so an interrupted or
  transiently-failed run can recover on the **next** run rather than hard-aborting.
- When a normal `brain sync` hits the `PriorListingMissing` abort (an incomplete
  baseline from a killed `--resync`), brain **automatically re-runs the resync**
  (the resume) instead of erroring out — uploading only what's missing and
  converging to a complete baseline. Journal records the auto-resume.
- brain **never journals `clean`/complete** unless the run finished with exit 0 and
  0 errors. An interrupted or errored run is `needs_attention`/`aborted`, so the
  next `brain sync` knows to finish the job.
- Net behavior: Ctrl-C at any point → re-run `brain sync` → it picks up where it
  left off and completes, with no file left behind.

## 4. Selective sync (keep giant files out)

- Extend `SyncConfig` (the env `sync` block) with:
  - `exclude: Vec<String>` — extra rclone exclude patterns (e.g. `**/test-data/**`),
    appended to the built-in excludes.
  - `max_size: String` — an rclone size string (e.g. `"100M"`; empty = no cap).
    When set, `args` adds `--max-size <cap>` so files above it are skipped.
- `args::bisync_args` appends `--exclude <p>` for each configured pattern and
  `--max-size <cap>` when non-empty.
- `brain sync setup` gains an optional step: offer a default max-size cap and let
  the user add exclude patterns (with the big-dir finding as motivation). Defaults
  stay permissive (no cap, no extra excludes) so behavior is unchanged unless opted
  into.
- **Scope note:** excludes and `max_size` are *deliberate omissions* — the never-miss
  guarantee (§3) applies to the in-scope set, not to files the user chose to skip.
  A later enhancement could surface skipped files in `brain sync status` so they're
  visible, not silently dropped.

## 5. Module touch-points

- `src/sync/run.rs` — the streaming runner (spawn + tee thread + capture); keep
  `parse_outcome` as-is (parses the captured buffer). This is the fiddly part
  (threaded pipe drain) and the main review focus.
- `src/sync/args.rs` — add `--stats`/`--stats-one-line`, `--resilient`/`--recover`,
  and the configured `--exclude`/`--max-size`. Pure; unit-tested.
- `src/sync/config.rs` — `exclude`, `max_size` fields (serde defaults; empty).
- `src/sync/command.rs` — auto-resume: on `AbortKind::PriorListingMissing`, re-run
  as `Direction::Resync` once and re-classify; journal the resume. Baseline-complete
  tracking.
- `src/sync/setup.rs` — optional size-cap / exclude prompts.
- Docs: `docs/features.md`, `docs/integrations.md`, `docs/decisions.md`,
  `docs/data-model.md`.

## 6. Testing (pure-first)

- `args`: `--stats-one-line`/`--stats` present; `--resilient`/`--recover` present;
  configured excludes each become `--exclude <p>`; `max_size` becomes `--max-size`
  only when set; empty config changes nothing.
- `config`: `exclude`/`max_size` parse + default empty.
- `command`: the auto-resume decision — a `PriorListingMissing` outcome triggers one
  resync retry; a clean/other outcome does not. (Pure decision extracted from the
  IO.)
- `run` streaming: keep `parse_outcome` unit tests; the tee/thread shell is
  exercised by the gated `tests/sync_local.rs` (which will now also see live output).
- Validate the real rclone flag spellings (`--stats-one-line`, `--resilient`,
  `--recover`, `--max-size`) against v1.74.2 at build time.

## 7. Acceptance

1. A real sync prints periodic progress (files/bytes/%/ETA) live, not a silent block.
2. Killing a sync mid-run and re-running `brain sync` resumes and completes without
   re-uploading matched files and without leaving any in-scope file un-synced.
3. brain never reports success for an interrupted/errored run.
4. `sync.exclude` / `sync.max_size` keep configured paths/large files out of the
   bucket; unset ⇒ unchanged behavior.
5. `cargo test --release` green; `cargo clippy --release --all-targets` clean;
   the gated integration test still passes with real rclone.

## 8. Decomposition (for the build)

- **P1 — args:** `--stats-one-line`/`--stats`, `--resilient`/`--recover`, plus
  `exclude`/`max_size` from config (pure, TDD).
- **P2 — config:** `exclude` + `max_size` fields (pure, TDD).
- **P3 — streaming runner:** `run.rs` spawn + tee + capture; live output, still
  parses. (The one thread-y piece; careful review.)
- **P4 — auto-resume:** `command.rs` PriorListingMissing → one resync retry;
  never-declare-clean-early; journal the resume.
- **P5 — setup:** optional size-cap / exclude prompts.
- **P6 — docs.**
