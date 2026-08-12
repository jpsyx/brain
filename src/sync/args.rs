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
/// Also every dependency tree (`node_modules/**`, at any depth) and the
/// machine-local package files an agent frontend drops beside its plugin.
/// OpenCode installs `@opencode-ai/plugin`'s dependencies into whatever
/// workspace it runs in, which is correct for OpenCode and pure waste to
/// transfer: it put 3.2k objects and 45 MiB on one remote and over half the
/// object count on another, and every machine rebuilds them for itself anyway.
/// OpenCode's own `.opencode/.gitignore` names exactly this set. Brain's
/// `.opencode/plugins/brain.js` bridge is *not* excluded — that is content every
/// machine needs.
const EXCLUDES: [&str; 20] = [
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
mod tests {
    use super::{SyncConfig, bisync_args, push_args};

    #[test]
    fn every_remote_walk_asks_for_one_recursive_listing() {
        // rclone's default march lists **per directory**, which on a bucket
        // backend is one API round trip per directory. Measured against a real
        // B2 workspace (6.7k objects, ~1k directories), `--fast-list` took a
        // dry-run bisync from 15.6s to 6.9s.
        let config = SyncConfig::default();
        for argv in [
            bisync_args(
                &config,
                "/local",
                "REMOTE:bucket",
                "/workdir",
                super::Direction::Both,
            ),
            push_args(&config, "/local", "REMOTE:bucket"),
        ] {
            assert!(
                argv.iter().any(|argument| argument == "--fast-list"),
                "{argv:?}"
            );
        }
    }
    use super::*;

    fn cfg() -> SyncConfig {
        serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","max_delete_percent":40}"#).unwrap()
    }

    fn args(dir: Direction) -> Vec<String> {
        bisync_args(&cfg(), "/root", "BRAIN:b", "/wd", dir)
    }

    #[test]
    fn pins_a_brain_owned_workdir_so_bisync_state_is_deterministic() {
        // brain owns rclone's bisync state dir: its location is fixed (not
        // rclone's HOME-dependent default) and its lock files are reapable.
        assert_eq!(pair_after(&args(Direction::Both), "--workdir"), Some("/wd"));
        assert_eq!(
            pair_after(&args(Direction::Resync), "--workdir"),
            Some("/wd")
        );
    }

    fn pair_after<'a>(v: &'a [String], flag: &str) -> Option<&'a str> {
        v.iter()
            .position(|s| s == flag)
            .and_then(|i| v.get(i + 1))
            .map(String::as_str)
    }

    #[test]
    fn both_resolves_newer_push_local_pull_remote() {
        assert_eq!(
            pair_after(&args(Direction::Both), "--conflict-resolve"),
            Some("newer")
        );
        assert_eq!(
            pair_after(&args(Direction::Push), "--conflict-resolve"),
            Some("path1")
        );
        assert_eq!(
            pair_after(&args(Direction::Pull), "--conflict-resolve"),
            Some("path2")
        );
    }

    #[test]
    fn automatic_push_uses_one_way_copy_instead_of_bisync() {
        let args = push_args(&cfg(), "/root", "BRAIN:b");
        assert_eq!(&args[..3], ["copy", "/root", "BRAIN:b"]);
        assert!(args.iter().any(|arg| arg == "--update"));
        assert!(!args.iter().any(|arg| arg == "bisync"));
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "--exclude" && pair[1] == "tasks/tasks.csv")
        );
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
        assert!(
            a.windows(2)
                .any(|w| w[0] == "--exclude" && w[1] == ".git/**")
        );
        assert!(
            a.windows(2)
                .any(|w| w[0] == "--exclude" && w[1] == "*(conflict *)*")
        );
        // Raw markers must be excluded so they don't re-propagate on later syncs.
        assert!(
            a.windows(2)
                .any(|w| w[0] == "--exclude" && w[1] == "*.__brainconflict__*")
        );
    }

    #[test]
    fn excludes_the_task_and_habit_csvs_so_they_merge_out_of_band() {
        // The two CSVs are merged via the 3-way merge, not bisynced, so they
        // must be excluded from the bisync argv.
        let a = args(Direction::Both);
        assert!(
            a.windows(2)
                .any(|w| w[0] == "--exclude" && w[1] == "tasks/tasks.csv"),
            "{a:?}"
        );
        assert!(
            a.windows(2)
                .any(|w| w[0] == "--exclude" && w[1] == "tasks/habits.csv"),
            "{a:?}"
        );
    }

    #[test]
    fn excludes_task_schema_metadata_for_schema_last_publication() {
        for argv in [args(Direction::Both), push_args(&cfg(), "/root", "BRAIN:b")] {
            assert!(
                argv.windows(2)
                    .any(|pair| pair[0] == "--exclude" && pair[1] == "tasks/SCHEMA.json"),
                "{argv:?}"
            );
        }
    }

    /// OpenCode installs its plugin's dependencies into the workspace it runs
    /// in, which put 3.2k files and 45 MiB of `node_modules` on one remote and
    /// over half the object count on another. They are machine-local build
    /// artifacts — OpenCode's own `.opencode/.gitignore` says exactly that — and
    /// every machine rebuilds them for itself.
    #[test]
    fn excludes_machine_local_agent_build_artifacts() {
        for argv in [args(Direction::Both), push_args(&cfg(), "/root", "BRAIN:b")] {
            for pattern in [
                "node_modules/**",
                ".opencode/package.json",
                ".opencode/package-lock.json",
                ".opencode/bun.lock",
                ".opencode/.gitignore",
            ] {
                assert!(
                    argv.windows(2)
                        .any(|pair| pair[0] == "--exclude" && pair[1] == pattern),
                    "missing exclude {pattern} in {argv:?}"
                );
            }
        }
    }

    /// The bridge Brain installs is content every machine needs, not an artifact.
    #[test]
    fn the_brain_opencode_plugin_itself_is_never_excluded() {
        for argv in [args(Direction::Both), push_args(&cfg(), "/root", "BRAIN:b")] {
            assert!(
                !argv
                    .windows(2)
                    .any(|pair| pair[0] == "--exclude" && pair[1].contains("plugins")),
                "{argv:?}"
            );
        }
    }

    #[test]
    fn excludes_the_workspace_manifest_after_identity_verification() {
        let bisync = args(Direction::Both);
        let push = push_args(&cfg(), "/root", "BRAIN:b");

        for argv in [&bisync, &push] {
            assert!(
                argv.windows(2)
                    .any(|pair| { pair[0] == "--exclude" && pair[1] == ".config/workspace.json" }),
                "{argv:?}"
            );
        }
    }

    #[test]
    fn excludes_setup_ownership_claims_from_portable_sync() {
        for argv in [args(Direction::Both), push_args(&cfg(), "/root", "BRAIN:b")] {
            assert!(
                argv.windows(2).any(|pair| {
                    pair[0] == "--exclude" && pair[1] == ".config/workspace-claims/**"
                }),
                "{argv:?}"
            );
        }
    }

    /// A crash-recovery journal is an instruction to *undo* a committed change,
    /// and it is only ever true of the machine that crashed. Transferred, the
    /// next machine to load portable users rolls its own file back to the
    /// journal's backup and then pushes that rollback outward, so one machine's
    /// interrupted `brain user` edit silently reverts the whole workspace's
    /// roster. Every in-root transaction artifact stays machine-local.
    #[test]
    fn excludes_transaction_journals_and_scratch_so_a_rollback_never_crosses_machines() {
        for argv in [args(Direction::Both), push_args(&cfg(), "/root", "BRAIN:b")] {
            for pattern in [".brain-*", "*.brain-triage-*", "*.transaction.lock"] {
                assert!(
                    argv.windows(2)
                        .any(|pair| pair[0] == "--exclude" && pair[1] == pattern),
                    "missing exclude {pattern} in {argv:?}"
                );
            }
        }
    }

    #[test]
    fn excludes_the_id_counters_so_they_max_merge_out_of_band() {
        // The id counters are max-merged out-of-band; bisync's newer-wins would
        // regress a counter and cause id collisions, so they must be excluded.
        let a = args(Direction::Both);
        assert!(
            a.windows(2)
                .any(|w| w[0] == "--exclude" && w[1] == "tasks/.tasks_next_id"),
            "{a:?}"
        );
        assert!(
            a.windows(2)
                .any(|w| w[0] == "--exclude" && w[1] == "tasks/.habits_next_id"),
            "{a:?}"
        );
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
        assert!(
            a.windows(2)
                .any(|w| w[0] == "--stats" && w[1].ends_with('s')),
            "{a:?}"
        );
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
        let a = bisync_args(&cfg, "/root", "BRAIN:b", "/wd", Direction::Both);
        assert!(
            a.windows(2)
                .any(|w| w[0] == "--exclude" && w[1] == "**/test-data/**"),
            "{a:?}"
        );
        assert!(
            a.windows(2).any(|w| w[0] == "--exclude" && w[1] == "*.mp4"),
            "{a:?}"
        );
        assert!(
            a.windows(2).any(|w| w[0] == "--max-size" && w[1] == "100M"),
            "{a:?}"
        );
    }

    #[test]
    fn omits_max_size_when_unset() {
        assert!(!args(Direction::Both).iter().any(|s| s == "--max-size"));
    }
}
