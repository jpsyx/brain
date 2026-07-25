//! Pure builder of the `rclone bisync` argument vector.
//!
//! Covers direction → conflict resolution bias, the keep-both conflict
//! flags, the default exclude filters, the `--max-delete` guard,
//! `--check-access`, and (for a baseline) `--resync`.

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
