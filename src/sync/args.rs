//! Pure builders for `rclone bisync` and the automatic one-way `rclone copy`.
//!
//! Covers direction → conflict resolution bias, the keep-both conflict
//! flags, the default exclude filters, the `--max-delete` guard, and (for a
//! baseline) `--resync`, and the `--check-access` marker guard.
//!
//! Also emits periodic one-line progress (`--stats 10s --stats-one-line`),
//! `--resilient`/`--recover` so an interrupted run can resume without a full
//! `--resync`, and config-driven `--exclude`/`--max-size` on top of the
//! built-in [`EXCLUDES`].

use crate::sync::config::SyncConfig;

/// Requested synchronization direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Newer edit wins (default `brain sync`).
    Both,
    /// Upload local additions and edits without downloading (`--push`).
    Push,
    /// Remote wins (`--pull`).
    Pull,
    /// (Re)establish the baseline (`brain sync repair`).
    Resync,
}

/// The marker suffix rclone appends to the conflict loser; `conflicts.rs`
/// rewrites it to the friendly `name (conflict host date).ext`.
pub const CONFLICT_MARKER: &str = "__brainconflict__";

/// Default excludes: the separately identity-gated portable workspace manifest
/// and setup ownership claims, VCS/OS cruft, the machine-local cache, friendly
/// conflict copies, raw rclone conflict markers (so neither fans out on later
/// syncs; the marker exclude does not stop rclone from creating the initial
/// copy), the two task CSVs
/// (`tasks/tasks.csv`, `tasks/habits.csv`) reconciled out-of-band via
/// the id-keyed 3-way merge in `csv_sync`, and the two id counters
/// (`tasks/.tasks_next_id`, `tasks/.habits_next_id`) reconciled out-of-band via
/// the max-merge in `counters` (bisync's newer-wins would regress a counter and
/// cause id collisions), not by bisync.
///
/// Also every in-root transaction artifact: the portable-user, triage-habit, and
/// task-schema journals plus their staged/backup/restore scratch (`.brain-*`, and
/// the `.<live-name>.brain-triage-…` siblings written beside a live file), and any
/// in-root transaction lock. A journal is an instruction to *undo* a committed
/// change and is only ever true of the machine that crashed: transferred, the next
/// machine to load that file rolls its own copy back to the journal's backup and
/// then pushes the rollback outward, so one interrupted edit reverts the whole
/// workspace. Locks are per-machine flocks with nothing portable in them.
///
/// Also every dependency tree (`node_modules/**`, at any depth), Python bytecode,
/// and the machine-local package files an agent frontend drops beside its plugin.
/// OpenCode installs `@opencode-ai/plugin`'s dependencies into whatever
/// workspace it runs in, which is correct for OpenCode and pure waste to
/// transfer: it put 3.2k objects and 45 MiB on one remote and over half the
/// object count on another, and every machine rebuilds them for itself anyway.
/// OpenCode's own `.opencode/.gitignore` names exactly this set. Brain's
/// `.opencode/plugins/brain.js` bridge is *not* excluded — that is content every
/// machine needs.
const EXCLUDES: [&str; 22] = [
    ".config/workspace.json",
    ".config/workspace-claims/**",
    // Unanchored, so each matches at any depth. `.brain-*` covers every journal
    // and its staged/backup/restore scratch; the triage pattern also catches the
    // siblings named after the live file (`.tasks.csv.brain-triage-<id>-0.staged`).
    ".brain-*",
    "*.brain-triage-*",
    "*.transaction.lock",
    ".git/**",
    ".DS_Store",
    ".cache/**",
    "*(conflict *)*",
    "*.__brainconflict__*",
    "tasks/tasks.csv",
    "tasks/habits.csv",
    "tasks/SCHEMA.json",
    "tasks/.tasks_next_id",
    "tasks/.habits_next_id",
    // Unanchored, so it matches at any depth: whatever tooling a workspace grows,
    // a dependency tree is rebuilt per machine and never worth transferring.
    "node_modules/**",
    "__pycache__/**",
    "*.pyc",
    ".opencode/package.json",
    ".opencode/package-lock.json",
    ".opencode/bun.lock",
    ".opencode/.gitignore",
];

/// Build the full argv for `rclone bisync <local> <remote>` for this direction.
#[must_use]
pub fn bisync_args(
    cfg: &SyncConfig,
    local: &str,
    remote_arg: &str,
    workdir: &str,
    dir: Direction,
) -> Vec<String> {
    let mut a: Vec<String> = vec!["bisync".into(), local.into(), remote_arg.into()];
    a.extend(["--workdir".into(), workdir.into()]);
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
    a.extend(["--check-filename".into(), "RCLONE_TEST".into()]);
    a.push("-v".into());
    a.extend(["--stats".into(), "10s".into()]);
    a.push("--stats-one-line".into());
    a.push("--resilient".into());
    a.push("--recover".into());
    // One recursive listing per side instead of rclone's default per-directory
    // march: on a bucket backend every directory level is otherwise its own API
    // round trip. Measured on a real B2 workspace (6.7k objects, ~1k
    // directories): a dry-run bisync went from 15.6s to 6.9s.
    a.push("--fast-list".into());
    for ex in EXCLUDES {
        a.extend(["--exclude".into(), ex.into()]);
    }
    for ex in &cfg.exclude {
        a.extend(["--exclude".into(), ex.clone()]);
    }
    if !cfg.max_size.trim().is_empty() {
        a.extend(["--max-size".into(), cfg.max_size.clone()]);
    }
    if dir == Direction::Resync {
        a.push("--resync".into());
    }
    a
}

/// Build a one-way, non-deleting automatic upload.
///
/// `copy --update` never downloads remote-only files and never removes them;
/// the next startup/message pull performs the full reconciliation.
#[must_use]
pub fn push_args(cfg: &SyncConfig, local: &str, remote_arg: &str) -> Vec<String> {
    let mut args = vec![
        "copy".to_owned(),
        local.to_owned(),
        remote_arg.to_owned(),
        "--update".to_owned(),
        "--fast-list".to_owned(),
        "-v".to_owned(),
        "--stats".to_owned(),
        "10s".to_owned(),
        "--stats-one-line".to_owned(),
    ];
    for exclude in EXCLUDES {
        args.extend(["--exclude".to_owned(), exclude.to_owned()]);
    }
    for exclude in &cfg.exclude {
        args.extend(["--exclude".to_owned(), exclude.clone()]);
    }
    if !cfg.max_size.trim().is_empty() {
        args.extend(["--max-size".to_owned(), cfg.max_size.clone()]);
    }
    args
}

#[cfg(test)]
mod tests;
