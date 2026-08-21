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
            "__pycache__/**",
            "*.pyc",
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
            argv.windows(2)
                .any(|pair| { pair[0] == "--exclude" && pair[1] == ".config/workspace-claims/**" }),
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
