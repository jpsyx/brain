# Brain Sync C2 — Sync core (manual) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `brain sync` — manual, bidirectional cross-machine sync of the brain directory to a private Backblaze B2 bucket via `rclone bisync`, with keep-both conflict copies, bidirectional deletes guarded by `--max-delete`, a SQLite sync journal, post-sync verification, and `brain sync setup`.

**Architecture:** A filled-out `src/sync/` module. Pure builders (`remote.rs`, `args.rs`, `conflicts.rs`, `verify.rs`) compute the rclone invocation, conflict-copy names, and run classification with zero IO and full unit coverage. Thin shells (`run.rs`, `journal.rs`, `setup.rs`, `command.rs`) invoke rclone, persist the journal, drive setup, and dispatch the CLI. Credentials reach rclone via `RCLONE_CONFIG_*` **environment variables** (never argv, never a persisted `rclone.conf`); all sync config stays in brain env (`~/.config/brain/env.json`). rclone is a soft prerequisite — absent it, `brain` runs fine and only `brain sync` degrades with a hint.

**Tech Stack:** Rust, `anyhow`, `serde`/`serde_json`, `rusqlite` (already a dep; WAL, mirrors `src/state.rs`), clap, external `rclone` binary via `std::process::Command`. `cargo test --release` + `cargo clippy --release --all-targets`.

---

## Scope

Phase **C2** of the [brain-sync spec](../specs/2026-07-24-brain-sync-design.md), detailed in the [C2 spec](../specs/2026-07-25-brain-sync-c2-sync-core.md). Builds on **C1** (brain env + parse-only `SyncConfig`, already merged to `main`). **In scope:** the manual `brain sync` command and all its machinery. **Out of scope:** the id-keyed CSV semantic merge (C3 — in C2 the CSVs ride the normal file lane with keep-both), auto/watcher triggers (C4), the `/second-brain` skill rows (C5). No new Rust crate.

## File Structure

| File | Responsibility | Pure? |
| --- | --- | --- |
| `src/sync/config.rs` *(exists, C1)* | `SyncConfig` | pure |
| `src/sync/remote.rs` (new) | `SyncConfig` → rclone `RCLONE_CONFIG_*` env + `BRAIN:<bucket>/<path>` arg | pure |
| `src/sync/args.rs` (new) | bisync arg vector: direction→conflict flags, excludes, `--max-delete`, `--check-access`, `--resync` | pure |
| `src/sync/run.rs` (new) | invoke rclone; parse summary → `RunOutcome`; detect aborts | thin IO + pure parse |
| `src/sync/conflicts.rs` (new) | friendly conflict-copy name builder + marker→friendly rewrite + on-disk enumerator | pure + thin IO |
| `src/sync/verify.rs` (new) | classify a run → `Outcome` (clean / needs-attention / aborted) | pure |
| `src/sync/journal.rs` (new) | SQLite `sync_runs` journal at `~/.cache/brain/sync/journal.db` | thin IO |
| `src/sync/setup.rs` (new) | `brain sync setup` flow | thin IO |
| `src/sync/command.rs` (new) | `brain sync` dispatch | thin |
| `src/sync/mod.rs` (edit) | wire submodules + `pub use command::run` | — |
| `src/lib.rs` / `src/main.rs` (edit) | `mod sync;` in the bin; `Sync` clap command; dispatch before the gate | — |
| `src/cli.rs` (edit) | `Sync(SyncArgs)` + `SyncAction` | — |
| `src/tasks/doctor.rs` (edit) | rclone/sync status line | — |
| `docs/*`, `AGENTS.md` (edit) | per spec §13 | — |

---

## C2.1 — Pure builders (`remote.rs`, `args.rs`)

### Task 1: `remote.rs` — build the rclone B2 remote from `SyncConfig`

**Files:** Create `src/sync/remote.rs`; edit `src/sync/mod.rs`.

- [ ] **Step 1: Write the failing test**

Create `src/sync/remote.rs`:

```rust
//! Pure builder: a `SyncConfig` → the rclone B2 remote, expressed as
//! `RCLONE_CONFIG_*` environment variables plus the `BRAIN:<bucket>/<path>`
//! argument. Credentials travel via env, never on argv (so they don't leak via
//! `ps`) and never in a persisted rclone.conf.

use crate::sync::config::SyncConfig;

/// The remote name used in both the env-var keys and the argv reference.
const REMOTE: &str = "BRAIN";

/// A fully-resolved rclone remote: the env vars that define it and the argv
/// token that references it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub env: Vec<(String, String)>,
    pub arg: String,
}

/// Build the B2 remote from sync config. `b2_path` is an optional prefix within
/// the bucket; a trailing slash is trimmed and an empty prefix is omitted.
#[must_use]
pub fn build_remote(cfg: &SyncConfig) -> Remote {
    let env = vec![
        (format!("RCLONE_CONFIG_{REMOTE}_TYPE"), "b2".to_owned()),
        (format!("RCLONE_CONFIG_{REMOTE}_ACCOUNT"), cfg.b2_key_id.clone()),
        (format!("RCLONE_CONFIG_{REMOTE}_KEY"), cfg.b2_app_key.clone()),
    ];
    let prefix = cfg.b2_path.trim().trim_matches('/');
    let arg = if prefix.is_empty() {
        format!("{REMOTE}:{}", cfg.b2_bucket.trim())
    } else {
        format!("{REMOTE}:{}/{prefix}", cfg.b2_bucket.trim())
    };
    Remote { env, arg }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SyncConfig {
        serde_json::from_str(
            r#"{"enabled":true,"b2_bucket":"my-brain","b2_key_id":"KID","b2_app_key":"AKEY"}"#,
        )
        .unwrap()
    }

    #[test]
    fn creds_go_in_env_never_in_the_arg() {
        let r = build_remote(&cfg());
        assert!(r.env.contains(&("RCLONE_CONFIG_BRAIN_TYPE".to_owned(), "b2".to_owned())));
        assert!(r.env.contains(&("RCLONE_CONFIG_BRAIN_ACCOUNT".to_owned(), "KID".to_owned())));
        assert!(r.env.contains(&("RCLONE_CONFIG_BRAIN_KEY".to_owned(), "AKEY".to_owned())));
        assert!(!r.arg.contains("KID") && !r.arg.contains("AKEY"));
    }

    #[test]
    fn arg_omits_an_empty_path_prefix() {
        assert_eq!(build_remote(&cfg()).arg, "BRAIN:my-brain");
    }

    #[test]
    fn arg_includes_and_trims_a_path_prefix() {
        let mut c = cfg();
        c.b2_path = "/sub/dir/".to_owned();
        assert_eq!(build_remote(&c).arg, "BRAIN:my-brain/sub/dir");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

First wire the module: in `src/sync/mod.rs` add `pub mod remote;`. Run: `cargo test --release sync::remote 2>&1 | tail -20`
Expected: compiles and the 3 tests pass (they're new; if the module isn't wired it won't compile — add `pub mod remote;`).

- [ ] **Step 3: Write minimal implementation** — done in Step 1.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --release sync::remote 2>&1 | tail -20` → PASS.

- [ ] **Step 5: Commit**

```bash
git add src/sync/remote.rs src/sync/mod.rs
git commit -m "feat(sync): pure rclone B2 remote builder (creds via env, not argv)"
```

### Task 2: `args.rs` — build the `rclone bisync` argument vector

**Files:** Create `src/sync/args.rs`; edit `src/sync/mod.rs`.

- [ ] **Step 1: Write the failing test**

Create `src/sync/args.rs`:

```rust
//! Pure builder of the `rclone bisync` argument vector: direction → conflict
//! resolution bias, the keep-both conflict flags, the default exclude filters,
//! the `--max-delete` guard, `--check-access`, and (for a baseline) `--resync`.

use crate::sync::config::SyncConfig;

/// Which side wins a same-file conflict on this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Newer edit wins (default `brain sync`).
    Both,
    /// Local wins (`--push`).
    Push,
    /// Remote wins (`--pull`).
    Pull,
    /// (Re)establish the baseline (`brain sync init` / `--resync`).
    Resync,
}

/// The marker suffix rclone appends to the conflict loser; `conflicts.rs`
/// rewrites it to the friendly `name (conflict host date).ext`.
pub const CONFLICT_MARKER: &str = "__brainconflict__";

/// Default excludes: VCS/OS cruft, the machine-local cache, and existing
/// conflict copies (so they never fan out).
const EXCLUDES: [&str; 4] = [".git/**", ".DS_Store", ".cache/**", "*(conflict *)*"];

/// Build the full argv for `rclone bisync <local> <remote>` for this direction.
#[must_use]
pub fn bisync_args(cfg: &SyncConfig, local: &str, remote_arg: &str, dir: Direction) -> Vec<String> {
    let mut a: Vec<String> = vec!["bisync".into(), local.into(), remote_arg.into()];
    let resolve = match dir {
        Direction::Both | Direction::Resync => "newer",
        Direction::Push => "path1",
        Direction::Pull => "path2",
    };
    a.extend(["--conflict-resolve".into(), resolve.into()]);
    a.extend(["--conflict-loser".into(), "pathname".into()]);
    a.extend(["--conflict-suffix".into(), CONFLICT_MARKER.into()]);
    a.extend(["--max-delete".into(), cfg.max_delete_percent.to_string()]);
    a.push("--check-access".into());
    for ex in EXCLUDES {
        a.extend(["--exclude".into(), ex.into()]);
    }
    if dir == Direction::Resync {
        a.push("--resync".into());
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SyncConfig {
        serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","max_delete_percent":40}"#).unwrap()
    }

    fn args(dir: Direction) -> Vec<String> {
        bisync_args(&cfg(), "/root", "BRAIN:b", dir)
    }

    fn pair_after<'a>(v: &'a [String], flag: &str) -> Option<&'a str> {
        v.iter().position(|s| s == flag).and_then(|i| v.get(i + 1)).map(String::as_str)
    }

    #[test]
    fn both_resolves_newer_push_local_pull_remote() {
        assert_eq!(pair_after(&args(Direction::Both), "--conflict-resolve"), Some("newer"));
        assert_eq!(pair_after(&args(Direction::Push), "--conflict-resolve"), Some("path1"));
        assert_eq!(pair_after(&args(Direction::Pull), "--conflict-resolve"), Some("path2"));
    }

    #[test]
    fn keeps_both_via_pathname_loser_and_marker_suffix() {
        let a = args(Direction::Both);
        assert_eq!(pair_after(&a, "--conflict-loser"), Some("pathname"));
        assert_eq!(pair_after(&a, "--conflict-suffix"), Some(CONFLICT_MARKER));
    }

    #[test]
    fn carries_max_delete_and_check_access_and_excludes() {
        let a = args(Direction::Both);
        assert_eq!(pair_after(&a, "--max-delete"), Some("40"));
        assert!(a.iter().any(|s| s == "--check-access"));
        assert!(a.windows(2).any(|w| w[0] == "--exclude" && w[1] == ".git/**"));
        assert!(a.windows(2).any(|w| w[0] == "--exclude" && w[1] == "*(conflict *)*"));
    }

    #[test]
    fn only_resync_adds_the_resync_flag() {
        assert!(args(Direction::Resync).iter().any(|s| s == "--resync"));
        assert!(!args(Direction::Both).iter().any(|s| s == "--resync"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Add `pub mod args;` to `src/sync/mod.rs`. Run: `cargo test --release sync::args 2>&1 | tail -20` → the 4 tests PASS once wired.

- [ ] **Step 3: Implementation** — done in Step 1.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/sync/args.rs src/sync/mod.rs
git commit -m "feat(sync): pure rclone bisync argument builder"
```

> **Implementation note (verify at build time):** confirm the installed rclone accepts `--conflict-resolve path1/path2/newer`, `--conflict-loser pathname`, and `--conflict-suffix` (rclone ≥ 1.66). If a flag spelling differs on the installed version, adjust `bisync_args` and its tests together — the builder shape is stable, only the literal flag strings may move.

---

## C2.2 — Run + parse (`run.rs`)

### Task 3: `run.rs` — invoke rclone and parse the outcome

**Files:** Create `src/sync/run.rs`; edit `src/sync/mod.rs`.

- [ ] **Step 1: Write the failing test** (parsing is the pure, tested core)

Create `src/sync/run.rs`:

```rust
//! Invoke `rclone` (thin `Command` shell) and parse its summary into a typed
//! `RunOutcome`. Only the parser is unit-tested; the process spawn is a thin
//! shell exercised via the integration path.

use std::process::Command;

/// Why a bisync aborted, when it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbortKind {
    /// `--max-delete` guard tripped.
    MaxDelete,
    /// Baseline listings missing — needs `brain sync init` / `--resync`.
    PriorListingMissing,
    /// Some other non-zero exit.
    Other,
}

/// Parsed result of one rclone run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub exit_ok: bool,
    pub transferred: u64,
    pub deleted: u64,
    pub errors: u64,
    pub abort: Option<AbortKind>,
}

/// Parse rclone's stderr/stdout text + exit success into a `RunOutcome`.
/// Defensive: unrecognized counts default to 0, but a non-zero exit with an
/// unrecognized reason is `AbortKind::Other` so verification treats it as
/// needs-attention rather than silently "clean".
#[must_use]
pub fn parse_outcome(exit_ok: bool, output: &str) -> RunOutcome {
    let count = |label: &str| -> u64 {
        output
            .lines()
            .find_map(|l| l.trim().strip_prefix(label))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|n| n.replace(',', "").parse().ok())
            .unwrap_or(0)
    };
    let lc = output.to_ascii_lowercase();
    let abort = if exit_ok {
        None
    } else if lc.contains("--max-delete") || lc.contains("max delete") {
        Some(AbortKind::MaxDelete)
    } else if lc.contains("cannot find prior") || lc.contains("must run --resync") || lc.contains("run --resync") {
        Some(AbortKind::PriorListingMissing)
    } else {
        Some(AbortKind::Other)
    };
    RunOutcome {
        exit_ok,
        transferred: count("Transferred:"),
        deleted: count("Deleted:"),
        errors: count("Errors:"),
        abort,
    }
}

/// Run `rclone <args>` with `env` injected, capturing combined output. Returns
/// the parsed outcome, or an `AbortKind::Other` outcome if rclone can't spawn.
pub fn run_rclone(env: &[(String, String)], args: &[String]) -> RunOutcome {
    let mut cmd = Command::new("rclone");
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    match cmd.output() {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            parse_outcome(out.status.success(), &text)
        }
        Err(_) => RunOutcome { exit_ok: false, transferred: 0, deleted: 0, errors: 0, abort: Some(AbortKind::Other) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_transfer_delete_error_counts() {
        let o = parse_outcome(true, "Transferred:   12 / 12, 100%\nDeleted:        3\nErrors:         0\n");
        assert_eq!((o.transferred, o.deleted, o.errors), (12, 3, 0));
        assert!(o.exit_ok && o.abort.is_none());
    }

    #[test]
    fn detects_max_delete_abort() {
        let o = parse_outcome(false, "ERROR: bisync aborting: --max-delete (50%) threshold exceeded");
        assert_eq!(o.abort, Some(AbortKind::MaxDelete));
    }

    #[test]
    fn detects_prior_listing_missing() {
        let o = parse_outcome(false, "Bisync error: cannot find prior Path1 or Path2 listings, likely due to critical error. Must run --resync");
        assert_eq!(o.abort, Some(AbortKind::PriorListingMissing));
    }

    #[test]
    fn unknown_nonzero_exit_is_other_not_clean() {
        assert_eq!(parse_outcome(false, "something went wrong").abort, Some(AbortKind::Other));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Add `pub mod run;` to `src/sync/mod.rs`. Run: `cargo test --release sync::run 2>&1 | tail -20` → 4 tests PASS once wired.

- [ ] **Step 3: Implementation** — done in Step 1.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/sync/run.rs src/sync/mod.rs
git commit -m "feat(sync): rclone invocation + defensive outcome parser"
```

> **Implementation note:** the fixture strings in the tests approximate rclone's summary format. At build time, run a real `rclone bisync --dry-run` against two local dirs and paste the actual `Transferred:/Deleted:/Errors:` lines and the real abort messages into the fixtures if they differ, adjusting `parse_outcome` to match. The defensive default (unknown → 0 counts, non-zero exit → `Other`) means a format drift degrades to "needs-attention", never a false "clean".

---

## C2.3 — Conflicts + verify

### Task 4: `conflicts.rs` — friendly conflict-copy names + enumeration

**Files:** Create `src/sync/conflicts.rs`; edit `src/sync/mod.rs`.

- [ ] **Step 1: Write the failing test**

Create `src/sync/conflicts.rs`:

```rust
//! Conflict-copy naming. rclone leaves the losing side of a same-file conflict
//! with the `args::CONFLICT_MARKER` suffix; we rewrite it to the friendly
//! `stem (conflict <host> <date>).ext`, and enumerate such copies for the
//! resolve flow (C5).

use std::path::{Path, PathBuf};

use crate::sync::args::CONFLICT_MARKER;

/// Build the friendly conflict name for an original path: insert
/// ` (conflict <host> <date>)` before the extension.
/// `note.md` → `note (conflict mac 2026-07-25).md`; an extensionless
/// `README` → `README (conflict mac 2026-07-25)`.
#[must_use]
pub fn conflict_name(original: &Path, host: &str, date: &str) -> PathBuf {
    let dir = original.parent();
    let stem = original.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = original.extension().map(|e| e.to_string_lossy().into_owned());
    let tag = format!("{stem} (conflict {host} {date})");
    let name = match ext {
        Some(e) => format!("{tag}.{e}"),
        None => tag,
    };
    match dir {
        Some(d) if !d.as_os_str().is_empty() => d.join(name),
        _ => PathBuf::from(name),
    }
}

/// Given a marker file rclone produced (`<original><MARKER>`), compute the
/// friendly path to rename it to. Returns `None` if the path doesn't carry the
/// marker suffix.
#[must_use]
pub fn friendly_from_marker(marker_path: &Path, host: &str, date: &str) -> Option<PathBuf> {
    let s = marker_path.to_string_lossy();
    let original = s.strip_suffix(CONFLICT_MARKER)?;
    Some(conflict_name(Path::new(original), host, date))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_conflict_tag_before_extension() {
        assert_eq!(
            conflict_name(Path::new("notes/idea.md"), "mac", "2026-07-25"),
            PathBuf::from("notes/idea (conflict mac 2026-07-25).md")
        );
    }

    #[test]
    fn handles_extensionless_files() {
        assert_eq!(
            conflict_name(Path::new("README"), "mac", "2026-07-25"),
            PathBuf::from("README (conflict mac 2026-07-25)")
        );
    }

    #[test]
    fn rewrites_a_marker_path_to_the_friendly_name() {
        let marker = PathBuf::from(format!("notes/idea.md{CONFLICT_MARKER}"));
        assert_eq!(
            friendly_from_marker(&marker, "mac", "2026-07-25"),
            Some(PathBuf::from("notes/idea (conflict mac 2026-07-25).md"))
        );
    }

    #[test]
    fn non_marker_path_yields_none() {
        assert_eq!(friendly_from_marker(Path::new("notes/idea.md"), "mac", "2026-07-25"), None);
    }
}
```

- [ ] **Step 2: Run** — add `pub mod conflicts;` to `src/sync/mod.rs`; `cargo test --release sync::conflicts 2>&1 | tail -20` → 4 tests PASS.
- [ ] **Step 3: Implementation** — done in Step 1.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/sync/conflicts.rs src/sync/mod.rs
git commit -m "feat(sync): conflict-copy name builder + marker rewrite"
```

### Task 5: the on-disk conflict post-pass + enumerator (thin IO)

**Files:** Edit `src/sync/conflicts.rs`.

- [ ] **Step 1: Write the failing test** (drive the rename via a temp dir)

Append to `src/sync/conflicts.rs`:

```rust
use std::fs;

/// Rename every `<path><MARKER>` file under `root` to its friendly conflict
/// name. Returns the count renamed. Best-effort: a failed rename is skipped.
pub fn rename_markers(root: &Path, host: &str, date: &str) -> usize {
    let mut n = 0;
    let walker = walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok);
    for entry in walker {
        let p = entry.path();
        if p.to_string_lossy().ends_with(CONFLICT_MARKER) {
            if let Some(dest) = friendly_from_marker(p, host, date) {
                if fs::rename(p, &dest).is_ok() {
                    n += 1;
                }
            }
        }
    }
    n
}

/// Count leftover marker files under `root` (used by verification).
#[must_use]
pub fn leftover_markers(root: &Path) -> usize {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().to_string_lossy().ends_with(CONFLICT_MARKER))
        .count()
}
```

Add a test in the same `tests` module:

```rust
    #[test]
    fn rename_markers_moves_marker_files_to_friendly_names() {
        let tmp = std::env::temp_dir().join(format!("brain-conflicts-{}", std::process::id()));
        let sub = tmp.join("notes");
        std::fs::create_dir_all(&sub).unwrap();
        let marker = sub.join(format!("idea.md{CONFLICT_MARKER}"));
        std::fs::write(&marker, b"loser").unwrap();

        assert_eq!(leftover_markers(&tmp), 1);
        let n = rename_markers(&tmp, "mac", "2026-07-25");
        assert_eq!(n, 1);
        assert_eq!(leftover_markers(&tmp), 0);
        assert!(sub.join("idea (conflict mac 2026-07-25).md").exists());

        std::fs::remove_dir_all(&tmp).ok();
    }
```

- [ ] **Step 2: Run** — `walkdir` is already a crate dependency (used by `entry.rs`). `cargo test --release sync::conflicts 2>&1 | tail -20` → new test PASSES (RED first if `rename_markers`/`leftover_markers` don't exist).
- [ ] **Step 3: Implementation** — done in Step 1.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/sync/conflicts.rs
git commit -m "feat(sync): rename rclone conflict markers to friendly names on disk"
```

### Task 6: `verify.rs` — classify a run

**Files:** Create `src/sync/verify.rs`; edit `src/sync/mod.rs`.

- [ ] **Step 1: Write the failing test**

Create `src/sync/verify.rs`:

```rust
//! Post-sync verification: turn a `run::RunOutcome` (+ a leftover-marker count)
//! into a final `Outcome` the journal and CLI report. A run is `Clean` only if
//! rclone exited cleanly with no errors and no un-renamed conflict markers
//! remain; anything else is surfaced.

use crate::sync::run::{AbortKind, RunOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Clean,
    NeedsAttention(String),
    Aborted(String),
}

impl Outcome {
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Clean => "clean",
            Outcome::NeedsAttention(_) => "needs_attention",
            Outcome::Aborted(_) => "aborted",
        }
    }
}

/// Classify a completed run. `leftover_markers` is the count of un-renamed
/// conflict markers found after the post-pass.
#[must_use]
pub fn classify(run: &RunOutcome, leftover_markers: usize) -> Outcome {
    if let Some(kind) = &run.abort {
        let msg = match kind {
            AbortKind::MaxDelete => "sync aborted: would delete more than the --max-delete threshold. If intentional, run `brain sync --resync`.",
            AbortKind::PriorListingMissing => "sync aborted: baseline listings missing. Run `brain sync init` to re-establish the baseline.",
            AbortKind::Other => "sync aborted: rclone exited with an error. See `brain sync status`.",
        };
        return Outcome::Aborted(msg.to_owned());
    }
    if run.errors > 0 {
        return Outcome::NeedsAttention(format!("{} transfer error(s); re-run `brain sync`.", run.errors));
    }
    if leftover_markers > 0 {
        return Outcome::NeedsAttention(format!("{leftover_markers} conflict copy(ies) could not be renamed; see `brain sync conflicts`."));
    }
    Outcome::Clean
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_run() -> RunOutcome {
        RunOutcome { exit_ok: true, transferred: 5, deleted: 1, errors: 0, abort: None }
    }

    #[test]
    fn clean_when_ok_no_errors_no_leftover_markers() {
        assert_eq!(classify(&ok_run(), 0), Outcome::Clean);
    }

    #[test]
    fn errors_are_needs_attention() {
        let mut r = ok_run();
        r.errors = 2;
        assert!(matches!(classify(&r, 0), Outcome::NeedsAttention(_)));
    }

    #[test]
    fn leftover_markers_are_needs_attention() {
        assert!(matches!(classify(&ok_run(), 1), Outcome::NeedsAttention(_)));
    }

    #[test]
    fn max_delete_abort_is_aborted_with_resync_hint() {
        let r = RunOutcome { exit_ok: false, transferred: 0, deleted: 0, errors: 0, abort: Some(AbortKind::MaxDelete) };
        match classify(&r, 0) {
            Outcome::Aborted(m) => assert!(m.contains("--resync")),
            other => panic!("expected Aborted, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run** — add `pub mod verify;` to `src/sync/mod.rs`; `cargo test --release sync::verify 2>&1 | tail -20` → 4 tests PASS.
- [ ] **Step 3: Implementation** — done in Step 1.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/sync/verify.rs src/sync/mod.rs
git commit -m "feat(sync): post-sync verification classifier"
```

---

## C2.4 — Journal (`journal.rs`)

### Task 7: the SQLite sync journal

**Files:** Create `src/sync/journal.rs`; edit `src/sync/mod.rs`.

- [ ] **Step 1: Write the failing test** (round-trip against an in-memory / temp DB, mirroring `src/state.rs`)

Create `src/sync/journal.rs`:

```rust
//! The sync journal: a small SQLite DB at `~/.cache/brain/sync/journal.db`
//! (machine-local cache, never synced) recording each run. Mirrors the WAL
//! setup of `crate::state`. The CSV-merge baselines (C3) will live beside it.

use std::path::PathBuf;

use anyhow::Result;
use rusqlite::Connection;

/// One recorded sync run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRun {
    pub started_at: String,
    pub finished_at: String,
    pub direction: String,
    pub outcome: String,
    pub transferred: u64,
    pub deleted: u64,
    pub conflicts: u64,
    pub errors: u64,
    pub note: String,
}

pub struct Journal {
    conn: Connection,
}

impl Journal {
    /// `~/.cache/brain/sync/journal.db`.
    #[must_use]
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("HOME")
            .map_or_else(|| PathBuf::from("."), |h| PathBuf::from(h).join(".cache").join("brain").join("sync"));
        base.join("journal.db")
    }

    /// Open (creating parent dirs + schema). WAL like the state DB.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::from_conn(conn)
    }

    fn from_conn(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sync_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at TEXT NOT NULL,
                finished_at TEXT NOT NULL,
                direction TEXT NOT NULL,
                outcome TEXT NOT NULL,
                transferred INTEGER NOT NULL,
                deleted INTEGER NOT NULL,
                conflicts INTEGER NOT NULL,
                errors INTEGER NOT NULL,
                note TEXT NOT NULL
            );",
        )?;
        Ok(Self { conn })
    }

    pub fn record(&self, r: &SyncRun) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_runs
               (started_at, finished_at, direction, outcome, transferred, deleted, conflicts, errors, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                r.started_at, r.finished_at, r.direction, r.outcome,
                r.transferred, r.deleted, r.conflicts, r.errors, r.note
            ],
        )?;
        Ok(())
    }

    /// Most-recent runs, newest first.
    pub fn recent(&self, limit: usize) -> Result<Vec<SyncRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT started_at, finished_at, direction, outcome, transferred, deleted, conflicts, errors, note
             FROM sync_runs ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |row| {
            Ok(SyncRun {
                started_at: row.get(0)?,
                finished_at: row.get(1)?,
                direction: row.get(2)?,
                outcome: row.get(3)?,
                transferred: row.get(4)?,
                deleted: row.get(5)?,
                conflicts: row.get(6)?,
                errors: row.get(7)?,
                note: row.get(8)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &str) -> SyncRun {
        SyncRun {
            started_at: "2026-07-25T00:00:00Z".into(),
            finished_at: "2026-07-25T00:00:05Z".into(),
            direction: dir.into(),
            outcome: "clean".into(),
            transferred: 3,
            deleted: 1,
            conflicts: 0,
            errors: 0,
            note: String::new(),
        }
    }

    fn mem() -> Journal {
        Journal::from_conn(Connection::open_in_memory().unwrap()).unwrap()
    }

    #[test]
    fn records_and_reads_back_newest_first() {
        let j = mem();
        j.record(&run("push")).unwrap();
        j.record(&run("pull")).unwrap();
        let got = j.recent(10).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].direction, "pull");
        assert_eq!(got[1].direction, "push");
    }

    #[test]
    fn default_path_is_under_cache_brain_sync() {
        assert!(Journal::default_path().ends_with(".cache/brain/sync/journal.db"));
    }
}
```

- [ ] **Step 2: Run** — add `pub mod journal;` to `src/sync/mod.rs`; `cargo test --release sync::journal 2>&1 | tail -20` → 2 tests PASS. (`from_conn` is `pub(self)`/private but reachable from the inline test module.)
- [ ] **Step 3: Implementation** — done in Step 1.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/sync/journal.rs src/sync/mod.rs
git commit -m "feat(sync): SQLite sync journal (~/.cache/brain/sync/journal.db)"
```

---

## C2.5 — Command + wiring

### Task 8: `command.rs` — orchestrate a sync run

**Files:** Create `src/sync/command.rs`; edit `src/sync/mod.rs`.

- [ ] **Step 1: Write the failing test** (the pure host/date helpers + the orchestration seam)

Create `src/sync/command.rs`:

```rust
//! `brain sync` orchestration. Ties the pure builders to the rclone shell, the
//! conflict post-pass, verification, and the journal. Kept thin; the tested
//! logic lives in the builders it calls.

use std::path::Path;

use anyhow::{Result, bail};

use crate::sync::args::{self, Direction};
use crate::sync::config::SyncConfig;
use crate::sync::conflicts;
use crate::sync::journal::{Journal, SyncRun};
use crate::sync::remote::build_remote;
use crate::sync::run::run_rclone;
use crate::sync::verify::{self, Outcome};

/// This machine's short hostname for conflict-copy names. Falls back to "host".
#[must_use]
pub fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
        })
        .map(|s| s.trim().split('.').next().unwrap_or("host").to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "host".to_owned())
}

/// Run one sync in `dir`. `now` supplies the timestamps + date (injected so the
/// call is testable and to keep clock reads out of pure code). Returns the
/// verified outcome.
pub fn sync_once(cfg: &SyncConfig, root: &Path, dir: Direction, now: (&str, &str, &str)) -> Result<Outcome> {
    if !cfg.is_configured() {
        bail!("sync is not configured — run `brain sync setup`");
    }
    let (started_at, finished_at, date) = now;
    let remote = build_remote(cfg);
    let local = root.to_string_lossy().into_owned();
    let argv = args::bisync_args(cfg, &local, &remote.arg, dir);

    let run = run_rclone(&remote.env, &argv);
    let renamed = conflicts::rename_markers(root, &hostname(), date) as u64;
    let leftover = conflicts::leftover_markers(root);
    let outcome = verify::classify(&run, leftover);

    let journal = Journal::open(&Journal::default_path())?;
    journal.record(&SyncRun {
        started_at: started_at.to_owned(),
        finished_at: finished_at.to_owned(),
        direction: direction_label(dir).to_owned(),
        outcome: outcome.label().to_owned(),
        transferred: run.transferred,
        deleted: run.deleted,
        conflicts: renamed,
        errors: run.errors,
        note: match &outcome {
            Outcome::Clean => String::new(),
            Outcome::NeedsAttention(m) | Outcome::Aborted(m) => m.clone(),
        },
    })?;
    Ok(outcome)
}

#[must_use]
pub fn direction_label(dir: Direction) -> &'static str {
    match dir {
        Direction::Both => "both",
        Direction::Push => "push",
        Direction::Pull => "pull",
        Direction::Resync => "resync",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_is_nonempty_and_unqualified() {
        let h = hostname();
        assert!(!h.is_empty());
        assert!(!h.contains('.'));
    }

    #[test]
    fn direction_labels_are_stable() {
        assert_eq!(direction_label(Direction::Both), "both");
        assert_eq!(direction_label(Direction::Resync), "resync");
    }

    #[test]
    fn sync_once_refuses_when_unconfigured() {
        let cfg: SyncConfig = serde_json::from_str("{}").unwrap();
        let err = sync_once(&cfg, Path::new("/tmp"), Direction::Both, ("a", "b", "2026-07-25")).unwrap_err();
        assert!(err.to_string().contains("brain sync setup"));
    }
}
```

- [ ] **Step 2: Run** — add `pub mod command;` to `src/sync/mod.rs`; `cargo test --release sync::command 2>&1 | tail -20` → 3 tests PASS. (The unconfigured test never spawns rclone; the configured path is exercised by the integration smoke in Task 12.)
- [ ] **Step 3: Implementation** — done in Step 1.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/sync/command.rs src/sync/mod.rs
git commit -m "feat(sync): sync_once orchestration (build → run → post-pass → verify → journal)"
```

### Task 9: the `brain sync` CLI surface + dispatch

**Files:** Edit `src/cli.rs`, `src/main.rs`, `src/lib.rs`, `src/sync/mod.rs`.

- [ ] **Step 1: Write the failing test**

In `src/sync/command.rs`, add a pure arg→direction classifier + its test:

```rust
/// Map the `--push`/`--pull` flags to a `Direction` for a bare `brain sync`.
#[must_use]
pub fn direction_from_flags(push: bool, pull: bool) -> Result<Direction> {
    match (push, pull) {
        (true, true) => bail!("--push and --pull are mutually exclusive"),
        (true, false) => Ok(Direction::Push),
        (false, true) => Ok(Direction::Pull),
        (false, false) => Ok(Direction::Both),
    }
}
```
```rust
    #[test]
    fn flags_map_to_direction() {
        assert_eq!(direction_from_flags(false, false).unwrap(), Direction::Both);
        assert_eq!(direction_from_flags(true, false).unwrap(), Direction::Push);
        assert_eq!(direction_from_flags(false, true).unwrap(), Direction::Pull);
        assert!(direction_from_flags(true, true).is_err());
    }
```
(`Direction` needs `#[derive(PartialEq, Eq)]` — it already has `Copy, Clone`; add `PartialEq, Eq` if missing.)

- [ ] **Step 2: Run** → `cargo test --release sync::command::tests::flags_map 2>&1 | tail -20` FAILS then PASSES.

- [ ] **Step 3: Wire the CLI.**

`src/cli.rs` — add the command beside `Env`:
```rust
    /// Sync your brain across machines via Backblaze B2 (`brain sync setup` first).
    Sync(SyncArgs),
```
```rust
#[derive(Args, Debug)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub action: Option<SyncAction>,
    /// Bias this run to the local side (local wins same-file conflicts).
    #[arg(long, global = true)]
    pub push: bool,
    /// Bias this run to the remote side (remote wins same-file conflicts).
    #[arg(long, global = true)]
    pub pull: bool,
}

#[derive(Subcommand, Debug)]
pub enum SyncAction {
    /// Configure the B2 bucket + credentials and establish the baseline.
    Setup,
    /// (Re)establish the bisync baseline (first run / recovery / fresh machine).
    Init,
    /// Show the last run, pending changes, and open conflicts.
    Status,
    /// List open conflict copies.
    Conflicts,
}
```

`src/lib.rs` — no change (sync already declared). `src/main.rs` — add `mod sync;` (C1 kept it lib-only), extend the `use crate::cli::{…}` import with `SyncAction, SyncArgs`, and dispatch before the gate (after the `Env` block):
```rust
    // `brain sync` needs neither the markdown-to-pdf prerequisite nor the TUI.
    if let Some(Cmd::Sync(args)) = &cli.command {
        return sync_command(args);
    }
```
Add the `unreachable!` arm beside the others: `Some(Cmd::Sync(_)) => unreachable!("sync is dispatched before the gate"),`

Add the handler (thin; timestamps come from `chrono::Utc`, already a dep via `chrono::Local`):
```rust
/// Handle `brain sync [--push|--pull] {setup|init|status|conflicts}`.
fn sync_command(args: &crate::cli::SyncArgs) -> Result<()> {
    use crate::sync::args::Direction;
    let cfg = crate::sync::config::SyncConfig::load();
    let root = paths::brain_root()?;
    match &args.action {
        Some(crate::cli::SyncAction::Setup) => crate::sync::setup::run(),
        Some(crate::cli::SyncAction::Init) => run_sync(&cfg, &root, Direction::Resync),
        Some(crate::cli::SyncAction::Status) => crate::sync::command::print_status(&cfg, &root),
        Some(crate::cli::SyncAction::Conflicts) => crate::sync::command::print_conflicts(&root),
        None => {
            let dir = crate::sync::command::direction_from_flags(args.push, args.pull)?;
            run_sync(&cfg, &root, dir)
        }
    }
}

/// Shared: run one sync and print the outcome.
fn run_sync(cfg: &crate::sync::config::SyncConfig, root: &std::path::Path, dir: crate::sync::args::Direction) -> Result<()> {
    let now = chrono::Utc::now();
    let ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    let outcome = crate::sync::command::sync_once(cfg, root, dir, (&ts, &ts, &date))?;
    match outcome {
        crate::sync::verify::Outcome::Clean => println!("sync complete."),
        crate::sync::verify::Outcome::NeedsAttention(m) | crate::sync::verify::Outcome::Aborted(m) => {
            eprintln!("{m}");
        }
    }
    Ok(())
}
```
(`print_status` / `print_conflicts` are added in Task 10; `setup::run` in Task 11. If you build before those exist, add thin `todo!()`-free stubs that print "not yet" — but sequence the tasks in order so stubs aren't needed.)

- [ ] **Step 4: Build + smoke** (unconfigured path is safe — it never spawns rclone):
```
cargo build --release 2>&1 | tail -3
./target/release/brain sync 2>&1 || true
```
Expected: prints `sync is not configured — run `brain sync setup`` (from `sync_once`'s guard) and exits non-zero, without touching the network.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/main.rs src/sync/command.rs
git commit -m "feat(sync): brain sync CLI surface + dispatch before the gate"
```

### Task 10: `brain sync status` + `conflicts` output

**Files:** Edit `src/sync/command.rs`, `src/sync/conflicts.rs`.

- [ ] **Step 1: Write the failing test** (pure formatting)

In `src/sync/conflicts.rs`, add the enumerator returning structured rows:
```rust
/// An open conflict copy found under the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFile {
    pub path: PathBuf,
}

/// List conflict copies (`*(conflict *)*`) under `root`, relative paths.
#[must_use]
pub fn list_conflicts(root: &Path) -> Vec<ConflictFile> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name().to_string_lossy();
            n.contains("(conflict ") && n.contains(')')
        })
        .map(|e| ConflictFile { path: e.path().strip_prefix(root).unwrap_or(e.path()).to_path_buf() })
        .collect()
}
```
In `src/sync/command.rs`, add a pure formatter + test:
```rust
/// Format the status line for the most recent journal run (pure).
#[must_use]
pub fn format_last_run(run: Option<&crate::sync::journal::SyncRun>) -> String {
    match run {
        None => "no syncs yet — run `brain sync`.".to_owned(),
        Some(r) => format!(
            "last sync: {} · {} · {} · {}↑ {}↓ {} conflicts{}",
            r.finished_at, r.direction, r.outcome, r.transferred, r.deleted, r.conflicts,
            if r.note.is_empty() { String::new() } else { format!(" · {}", r.note) },
        ),
    }
}
```
```rust
    #[test]
    fn format_last_run_handles_empty_and_populated() {
        assert!(format_last_run(None).contains("no syncs yet"));
        let r = crate::sync::journal::SyncRun {
            started_at: "s".into(), finished_at: "2026-07-25T00:00:05Z".into(),
            direction: "both".into(), outcome: "clean".into(),
            transferred: 3, deleted: 1, conflicts: 0, errors: 0, note: String::new(),
        };
        let line = format_last_run(Some(&r));
        assert!(line.contains("both") && line.contains("clean") && line.contains("3↑"));
    }
```

- [ ] **Step 2: Run** → RED then GREEN for `format_last_run` and (compile) `list_conflicts`.

- [ ] **Step 3: Add the thin printers** in `src/sync/command.rs`:
```rust
/// Print `brain sync status`.
pub fn print_status(cfg: &SyncConfig, root: &Path) -> Result<()> {
    if !cfg.is_configured() {
        println!("sync is not configured — run `brain sync setup`.");
        return Ok(());
    }
    let journal = Journal::open(&Journal::default_path())?;
    let recent = journal.recent(1)?;
    println!("{}", format_last_run(recent.first()));
    let conflicts = conflicts::list_conflicts(root);
    println!("open conflicts: {}", conflicts.len());
    Ok(())
}

/// Print `brain sync conflicts`.
pub fn print_conflicts(root: &Path) -> Result<()> {
    let conflicts = conflicts::list_conflicts(root);
    if conflicts.is_empty() {
        println!("no open conflict copies.");
    } else {
        for c in conflicts {
            println!("{}", c.path.display());
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run** → `cargo test --release sync:: 2>&1 | tail -12` all PASS; `cargo clippy --release --all-targets` clean.

- [ ] **Step 5: Commit**

```bash
git add src/sync/command.rs src/sync/conflicts.rs
git commit -m "feat(sync): brain sync status + conflicts output"
```

---

## C2.6 — Setup + doctor

### Task 11: `setup.rs` — `brain sync setup`

**Files:** Create `src/sync/setup.rs`; edit `src/sync/mod.rs`.

- [ ] **Step 1: Write the failing test** (pure input validation)

Create `src/sync/setup.rs` with a pure validator + interactive shell:

```rust
//! `brain sync setup`: check rclone, collect the B2 bucket + credentials into
//! the brain-env `sync` block, verify/create the bucket, and establish the
//! baseline. Interactive on /dev/tty; the validation is pure and tested.

use anyhow::{Result, bail};

/// Validate collected setup inputs before writing them to env. Pure.
pub fn validate(bucket: &str, key_id: &str, app_key: &str) -> Result<()> {
    if bucket.trim().is_empty() {
        bail!("bucket name is required");
    }
    if key_id.trim().is_empty() || app_key.trim().is_empty() {
        bail!("both a B2 key ID and application key are required");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_fields() {
        assert!(validate("", "k", "a").is_err());
        assert!(validate("b", "", "a").is_err());
        assert!(validate("b", "k", "").is_err());
        assert!(validate("b", "k", "a").is_ok());
    }
}
```

- [ ] **Step 2: Run** — add `pub mod setup;` to `src/sync/mod.rs`; `cargo test --release sync::setup 2>&1 | tail -20` → PASS.

- [ ] **Step 3: Add the interactive `run()`** (thin IO; reads /dev/tty like the personalization onboarding does — mirror `src/personalization/onboarding.rs` for the prompt helper):
```rust
/// Interactive setup. Writes the `sync` block into brain env, verifies/creates
/// the bucket, and runs the initial baseline sync.
pub fn run() -> Result<()> {
    if which_rclone().is_none() {
        eprintln!("rclone is not installed. Install it (https://rclone.org/downloads/) and re-run `brain sync setup`.");
        return Ok(());
    }
    // Prompt bucket / key id / app key on /dev/tty (Enter keeps existing).
    // (Reuse the prompt helper pattern from personalization::onboarding.)
    let existing = crate::sync::config::SyncConfig::load();
    let bucket = prompt("B2 bucket", &existing.b2_bucket)?;
    let key_id = prompt("B2 key ID", &existing.b2_key_id)?;
    let app_key = prompt("B2 application key", &existing.b2_app_key)?;
    validate(&bucket, &key_id, &app_key)?;

    write_sync_block(&bucket, &key_id, &app_key)?;
    verify_or_create_bucket(&bucket, &key_id, &app_key)?;
    println!("Establishing the baseline (this may take a while)…");
    let cfg = crate::sync::config::SyncConfig::load();
    let root = crate::paths::brain_root()?;
    let now = chrono::Utc::now();
    let ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    crate::sync::command::sync_once(&cfg, &root, crate::sync::args::Direction::Resync, (&ts, &ts, &date))?;
    println!("sync configured.");
    Ok(())
}

fn which_rclone() -> Option<std::path::PathBuf> {
    std::process::Command::new("rclone").arg("version").output().ok().and_then(|o| o.status.success().then(|| std::path::PathBuf::from("rclone")))
}
```
Implement `prompt` (read a line from `/dev/tty`, default-on-empty), `write_sync_block` (build the `sync` JSON object and store it via the env store — set `enabled=true`, the bucket, key id, app key; `b2_path` empty), and `verify_or_create_bucket` (`rclone lsd BRAIN:<bucket>` with env creds; on failure offer `rclone mkdir`). Keep each thin; they have no pure logic beyond `validate`.

> **Env write detail:** the `sync` block is a nested object. Add a small helper to the env store (or reuse a JSON set path) to write `sync` as an object rather than a scalar — the existing `env::set` coerces scalars. Add `env::set_json(name, serde_json::Value)` (thin, mirrors `env::set`) if not present, and unit-test it round-trips an object.

- [ ] **Step 4: Build + smoke** (do NOT complete a real setup here — just confirm the rclone-absent branch and the dispatch compile/run):
```
cargo build --release 2>&1 | tail -3
# With rclone possibly installed, `brain sync setup` would prompt; don't run it
# interactively in CI. Confirm the binary builds and `brain sync status` works:
./target/release/brain sync status 2>&1 || true
```

- [ ] **Step 5: Commit**

```bash
git add src/sync/setup.rs src/sync/mod.rs src/env/*.rs
git commit -m "feat(sync): brain sync setup (collect creds → env, verify bucket, baseline)"
```

### Task 12: doctor rclone line + integration smoke

**Files:** Edit `src/tasks/doctor.rs`; add `tests/sync_local.rs` (gated).

- [ ] **Step 1: Write the failing test** (doctor line is pure over inputs)

In `src/tasks/doctor.rs`, extend the `Diagnosis` with an rclone/sync field and a pure formatter for it, plus a test asserting the line reflects "installed + configured" vs "missing". Mirror the existing doctor checks' structure (read the file first; follow its `Diagnosis` shape and its test style).

- [ ] **Step 2–4: Implement + run** the doctor line: detect rclone presence (`which_rclone`) and whether `SyncConfig::load().is_configured()`, format one line (e.g. `rclone: ✓ 1.66  · sync: configured (bucket my-brain)` or `rclone: ✗ not installed · sync: off`). Full suite green, clippy clean.

- [ ] **Step 5 (optional, gated): a local-backend integration test.** Add `tests/sync_local.rs` that, **only if `rclone` is on PATH**, bisyncs two temp dirs through rclone's local backend (no B2), asserting a create propagates and a delete propagates. Guard the whole test body on `Command::new("rclone").arg("version")` succeeding, so the default suite passes on machines without rclone. This exercises `run_rclone` + the post-pass end-to-end without network or credentials.

- [ ] **Step 6: Commit**

```bash
git add src/tasks/doctor.rs tests/sync_local.rs
git commit -m "feat(sync): doctor rclone/sync line + gated local-backend integration test"
```

---

## C2.7 — Docs

### Task 13: documentation

**Files:** `docs/features.md`, `docs/integrations.md`, `docs/architecture.md`, `docs/data-model.md`, `docs/config.md`, `docs/decisions.md`, `AGENTS.md`.

- [ ] **Step 1: Full green + clippy**

Run: `cargo test --release 2>&1 | tail -8 && cargo clippy --release --all-targets 2>&1 | grep -cE "warning:"`
Expected: all pass; 0 warnings.

- [ ] **Step 2: `docs/features.md`** — `brain sync [--push|--pull] {setup|init|status|conflicts}`; keep-both conflict behavior; the doctor rclone line.

- [ ] **Step 3: `docs/integrations.md`** — the rclone/B2 handoff: env-var creds (no rclone.conf, no secrets on argv), `--max-delete`/`--check-access`, the setup flow, and the journal at `~/.cache/brain/sync/journal.db`.

- [ ] **Step 4: `docs/architecture.md`** — the `src/sync/` module map and the build→run→post-pass→verify→journal flow; `Sync` dispatched before the gate; rclone as an external dependency.

- [ ] **Step 5: `docs/data-model.md`** — the `sync_runs` schema; the `name (conflict <host> <date>).ext` convention.

- [ ] **Step 6: `docs/config.md`** — `brain sync setup` writes the env `sync` block; rclone as a soft prerequisite (not a startup gate).

- [ ] **Step 7: `docs/decisions.md`** — secrets-via-env-not-argv; env-var remote over a persisted rclone.conf; `--max-delete`/`--check-access` guards; no silent auto-resync.

- [ ] **Step 8: `AGENTS.md` docs-contract table** — add `brain sync` rows (command, rclone transport, journal) pointing at `src/sync/`.

- [ ] **Step 9: Commit**

```bash
git add docs/ AGENTS.md
git commit -m "docs: brain sync (C2) — command, rclone transport, journal"
```

---

## Self-Review

**Spec coverage (C2 slice):**
- C2 spec §3 module layout → Tasks 1–11 (one file per responsibility).
- §4 command surface (`setup|init|status|conflicts`, `--push`/`--pull`) → Tasks 9, 10, 11.
- §5 transport (env-var remote, filters, `--resync`, `--check-access`) → Tasks 1, 2, 11.
- §6 keep-both conflicts (marker → friendly rename, enumerator) → Tasks 4, 5, 10.
- §7 deletions + `--max-delete` → Task 2 (arg) + Task 6 (abort classification).
- §8 verification + journal → Tasks 6, 7.
- §9 setup → Task 11.
- §10 secrets machine-local/off-argv → Tasks 1 (env not argv), 11 (env write).
- §11 recovery (`init`, prior-listing-missing, max-delete) → Tasks 3 (detect), 6 (message), 9 (`init`).
- §13 docs → Task 13.
- Out of C2 scope (CSV semantic merge, watcher/triggers, skill rows) — absent; C3–C5.

**Placeholder scan:** No TBD/TODO in shipped code. Two `> Implementation note` callouts (Tasks 2, 3) flag that rclone flag spellings and summary-output fixtures must be validated against the installed rclone — this is a real, spec-acknowledged verification step (spec §16), not a placeholder; the builders/parsers are complete and defensive. Task 9's note about `print_status`/`setup::run` ordering is resolved by executing tasks in order.

**Type consistency:** `SyncConfig`, `Remote`/`build_remote`, `Direction`/`bisync_args`/`CONFLICT_MARKER`, `RunOutcome`/`AbortKind`/`parse_outcome`/`run_rclone`, `conflict_name`/`friendly_from_marker`/`rename_markers`/`leftover_markers`/`list_conflicts`/`ConflictFile`, `Outcome`/`classify`, `Journal`/`SyncRun`, `sync_once`/`hostname`/`direction_label`/`direction_from_flags`/`format_last_run`/`print_status`/`print_conflicts`, `setup::run`/`validate` are used with consistent names/signatures across tasks. `Direction` gets `PartialEq, Eq` in Task 9 (needed by its test and by `bisync_args` matches).

**Ordering:** Execute in order. Task 8 (`sync_once`) depends on Tasks 1–7; Task 9 (dispatch) depends on 8; Tasks 10–11 add functions `sync_command` references, so they land before the final build in Task 12/13. `mod sync;` is added to `main.rs` in Task 9 (C1 left it lib-only); every other `pub mod` in `src/sync/mod.rs` is added in the task that creates the file.

**Prerequisite:** C1 must be merged to `main` (it is). C2 assumes brain env, `SyncConfig`, `env::set`, and `paths::brain_root()` exist.
