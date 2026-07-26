//! Pure builder of the `rclone bisync` argument vector.
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

/// Default excludes: VCS/OS cruft, the machine-local cache, friendly conflict
/// copies, raw rclone conflict markers (so neither fans out on later syncs; the
/// marker exclude does not stop rclone from creating the initial copy), and the
/// two task CSVs (`tasks/tasks.csv`, `tasks/habits.csv`) which are reconciled
/// out-of-band via the id-keyed 3-way merge in `csv_sync`, not by bisync.
const EXCLUDES: [&str; 7] = [
    ".git/**",
    ".DS_Store",
    ".cache/**",
    "*(conflict *)*",
    "*.__brainconflict__*",
    "tasks/tasks.csv",
    "tasks/habits.csv",
];

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
    a.extend(["--check-filename".into(), "RCLONE_TEST".into()]);
    a.push("-v".into());
    a.extend(["--stats".into(), "10s".into()]);
    a.push("--stats-one-line".into());
    a.push("--resilient".into());
    a.push("--recover".into());
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
    fn carries_max_delete_check_access_and_excludes() {
        let a = args(Direction::Both);
        assert_eq!(pair_after(&a, "--max-delete"), Some("40"));
        assert!(a.iter().any(|s| s == "--check-access"));
        assert_eq!(pair_after(&a, "--check-filename"), Some("RCLONE_TEST"));
        assert!(a.windows(2).any(|w| w[0] == "--exclude" && w[1] == ".git/**"));
        assert!(a.windows(2).any(|w| w[0] == "--exclude" && w[1] == "*(conflict *)*"));
        // Raw markers must be excluded so they don't re-propagate on later syncs.
        assert!(a.windows(2).any(|w| w[0] == "--exclude" && w[1] == "*.__brainconflict__*"));
    }

    #[test]
    fn excludes_the_task_and_habit_csvs_so_they_merge_out_of_band() {
        // The two CSVs are merged via the 3-way merge, not bisynced, so they
        // must be excluded from the bisync argv.
        let a = args(Direction::Both);
        assert!(a.windows(2).any(|w| w[0] == "--exclude" && w[1] == "tasks/tasks.csv"), "{a:?}");
        assert!(a.windows(2).any(|w| w[0] == "--exclude" && w[1] == "tasks/habits.csv"), "{a:?}");
    }

    #[test]
    fn passes_verbose_so_rclone_emits_the_summary_block() {
        // Without -v rclone prints no `Transferred:`/`Deleted:` summary at
        // default verbosity, so the parser would always read 0 counts.
        assert!(args(Direction::Both).iter().any(|s| s == "-v"));
    }

    #[test]
    fn only_resync_adds_the_resync_flag() {
        assert!(args(Direction::Resync).iter().any(|s| s == "--resync"));
        assert!(!args(Direction::Both).iter().any(|s| s == "--resync"));
    }

    #[test]
    fn emits_periodic_one_line_progress() {
        let a = args(Direction::Both);
        assert!(a.iter().any(|s| s == "--stats-one-line"), "{a:?}");
        // --stats has a duration value
        assert!(a.windows(2).any(|w| w[0] == "--stats" && w[1].ends_with('s')), "{a:?}");
    }

    #[test]
    fn is_resilient_for_resumable_interrupted_runs() {
        assert!(args(Direction::Both).iter().any(|s| s == "--resilient"));
    }

    #[test]
    fn appends_configured_excludes_and_max_size() {
        let cfg: SyncConfig = serde_json::from_str(
            r#"{"enabled":true,"b2_bucket":"b","exclude":["**/test-data/**","*.mp4"],"max_size":"100M"}"#,
        )
        .unwrap();
        let a = bisync_args(&cfg, "/root", "BRAIN:b", Direction::Both);
        assert!(a.windows(2).any(|w| w[0] == "--exclude" && w[1] == "**/test-data/**"), "{a:?}");
        assert!(a.windows(2).any(|w| w[0] == "--exclude" && w[1] == "*.mp4"), "{a:?}");
        assert!(a.windows(2).any(|w| w[0] == "--max-size" && w[1] == "100M"), "{a:?}");
    }

    #[test]
    fn omits_max_size_when_unset() {
        assert!(!args(Direction::Both).iter().any(|s| s == "--max-size"));
    }
}
