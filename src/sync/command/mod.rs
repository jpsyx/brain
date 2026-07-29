//! `brain sync` orchestration.
//!
//! Ties the pure builders to the rclone shell, the conflict post-pass,
//! verification, and the journal. Kept thin; the tested logic lives in the
//! builders it calls. The `resolve` submodule (`brain sync resolve`) is
//! self-contained enough to live in its own file; its one externally-called
//! entry point is re-exported here so `crate::sync::command::resolve` (called
//! from `main.rs`) keeps resolving unchanged. `ResolveDecision`/
//! `resolve_decision` have no call sites outside `resolve.rs` itself, so they
//! stay at their natural `resolve::` path rather than being re-exported
//! (which `cargo clippy` would flag as unused in this binary crate).

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};

use crate::sync::args::{self, Direction};
use crate::sync::config::SyncConfig;
use crate::sync::conflicts::{self, ConflictGroup};
use crate::sync::csv_sync::CsvMergeOutcome;
use crate::sync::journal::{Journal, SyncRun};
use crate::sync::remote::{Remote, build_remote};
use crate::sync::run::run_rclone;
use crate::sync::verify::{self, Outcome};
use crate::theme::Theme;

mod resolve;
pub use resolve::resolve;

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
pub fn sync_once(
    cfg: &SyncConfig,
    root: &Path,
    dir: Direction,
    now: (&str, &str, &str),
) -> Result<Outcome> {
    if !cfg.is_configured() {
        bail!("sync is not configured — run `brain sync setup`");
    }
    let (started_at, finished_at, date) = now;
    let remote = build_remote(cfg);
    let local = root.to_string_lossy().into_owned();
    let workdir = crate::sync::run::bisync_workdir();
    let _ = std::fs::create_dir_all(&workdir);
    let workdir_arg = workdir.to_string_lossy().into_owned();
    let argv = args::bisync_args(cfg, &local, &remote.arg, &workdir_arg, dir);
    let theme = Theme::active();
    // The single output sink for this run: everything below is mirrored to
    // `current.log` (so a following `brain sync` and `brain sync status` can
    // observe a detached background sync) and echoed to this process's stderr.
    let reporter =
        crate::sync::current::Reporter::begin(direction_label(dir), started_at, std::process::id());
    crate::logging::log(format!(
        "sync_once direction={} root={} remote={}",
        direction_label(dir),
        root.display(),
        remote.arg
    ));
    reporter.line(&format_sync_plan(cfg, root, dir, theme));

    if !crate::sync::run::rclone_present() {
        return Ok(Outcome::Aborted(crate::sync::run::missing_rclone_guidance(
            theme,
            "brain sync",
        )));
    }

    reporter.line(&theme.info(sync_progress(dir)));

    if should_bootstrap_check_access(dir) {
        crate::logging::log("sync check-access markers");
        reporter.line(&theme.info("Checking the sync safety marker…"));
        crate::sync::check_access::ensure_markers(root, &remote)?;
    }

    // We hold brain's machine-wide sync lock here, so any rclone bisync lock
    // file in the workdir is from a dead, interrupted run — reap it so an
    // earlier crash (TUI quit, power off) never wedges this run.
    crate::sync::run::reap_stale_bisync_locks(&workdir);
    crate::logging::log("sync rclone start");
    reporter.line(&theme.info("Starting rclone sync; live file progress follows…"));
    let mut run = run_rclone(&reporter, &remote.env, &argv);
    crate::logging::log(format!(
        "sync rclone done exit_ok={} transferred={} deleted={} errors={} abort={:?}",
        run.exit_ok, run.transferred, run.deleted, run.errors, run.abort
    ));
    let resumed = if should_auto_resync(dir, run.abort.as_ref()) {
        crate::logging::log("sync auto-resync start");
        reporter.line(&theme.warning(
            "rclone reported that its baseline listing is incomplete; establishing it with a one-time resync…",
        ));
        let resync_argv =
            args::bisync_args(cfg, &local, &remote.arg, &workdir_arg, Direction::Resync);
        run = run_rclone(&reporter, &remote.env, &resync_argv);
        crate::logging::log(format!(
            "sync auto-resync done exit_ok={} transferred={} deleted={} errors={} abort={:?}",
            run.exit_ok, run.transferred, run.deleted, run.errors, run.abort
        ));
        true
    } else {
        false
    };
    let auto_repaired = if should_auto_repair_check_access(dir, run.abort.as_ref()) {
        crate::logging::log("sync auto-repair check-access marker");
        reporter.line(&theme.warning(
            "The check-access marker is missing; running `brain sync repair` automatically to recreate it and re-establish the baseline…",
        ));
        reporter.line(&theme.info("Recreating the local and remote RCLONE_TEST markers…"));
        crate::sync::check_access::ensure_markers(root, &remote)?;
        reporter.line(&theme.info("Rebuilding the rclone baseline; live file progress follows…"));
        let repair_argv =
            args::bisync_args(cfg, &local, &remote.arg, &workdir_arg, Direction::Resync);
        run = run_rclone(&reporter, &remote.env, &repair_argv);
        crate::logging::log(format!(
            "sync auto-repair done exit_ok={} transferred={} deleted={} errors={} abort={:?}",
            run.exit_ok, run.transferred, run.deleted, run.errors, run.abort
        ));
        true
    } else {
        false
    };
    crate::logging::log("sync rename conflict markers");
    let renamed_count = conflicts::rename_markers(root, &hostname(), date);
    let renamed = u64::try_from(renamed_count).unwrap_or(0);
    let leftover = conflicts::leftover_markers(root);
    let outcome = verify::classify(&run, renamed_count, leftover);
    crate::logging::log(format!(
        "sync verify outcome={} renamed_conflicts={} leftover_markers={}",
        outcome.label(),
        renamed_count,
        leftover
    ));

    // The two task CSVs are excluded from bisync and reconciled out-of-band via
    // the 3-way merge. Best-effort: skip on an abort, and never let a CSV
    // failure change the bisync outcome — just record what merged.
    let csv_note = if matches!(outcome, Outcome::Aborted(_)) {
        crate::logging::log("sync csv merge skipped after abort");
        String::new()
    } else {
        crate::logging::log("sync csv merge start");
        reporter.line(&theme.info("Merging task and habit CSVs by row id…"));
        let note = format_csv_note(&crate::sync::csv_sync::sync_csvs(cfg, root));
        crate::logging::log(format!("sync csv merge note={note:?}"));
        // Reconcile the monotonic id counters by max, so neither machine ever
        // reuses an id the other already handed out. Best-effort, like the CSVs.
        reporter.line(&theme.info("Reconciling task and habit id counters…"));
        let counters = crate::sync::counters::sync_counters(cfg, root);
        crate::logging::log(format!("sync id counters {counters:?}"));
        note
    };

    let journal = Journal::open(&Journal::default_path())?;
    crate::logging::log(format!(
        "sync journal {}",
        Journal::default_path().display()
    ));
    journal.record(&SyncRun {
        started_at: started_at.to_owned(),
        finished_at: finished_at.to_owned(),
        direction: direction_label(dir).to_owned(),
        outcome: outcome.label().to_owned(),
        transferred: run.transferred,
        deleted: run.deleted,
        conflicts: renamed,
        errors: run.errors,
        note: {
            let base = match &outcome {
                Outcome::Clean => String::new(),
                Outcome::NeedsAttention(m) | Outcome::Aborted(m) => m.clone(),
            };
            let base = if resumed {
                if base.is_empty() {
                    "auto-resumed after interrupted baseline".to_owned()
                } else {
                    format!("auto-resumed after interrupted baseline; {base}")
                }
            } else {
                base
            };
            let base = if auto_repaired {
                if base.is_empty() {
                    "auto-repaired missing check-access marker".to_owned()
                } else {
                    format!("auto-repaired missing check-access marker; {base}")
                }
            } else {
                base
            };
            join_notes(&base, &csv_note)
        },
    })?;
    crate::logging::log("sync journal recorded");
    Ok(outcome)
}

/// Summarize the CSV merge outcomes into a journal note segment, e.g.
/// `csv: +3 ~2 -1 (1 soft)`. Empty when nothing was added, merged, deleted, or
/// soft-conflicted, so a clean run stays noise-free.
#[must_use]
pub fn format_csv_note(outcomes: &[CsvMergeOutcome]) -> String {
    let (added, merged, deleted, soft) = outcomes.iter().fold((0, 0, 0, 0), |acc, o| {
        (
            acc.0 + o.added,
            acc.1 + o.merged,
            acc.2 + o.deleted,
            acc.3 + o.soft_conflicts,
        )
    });
    if added == 0 && merged == 0 && deleted == 0 && soft == 0 {
        return String::new();
    }
    let mut note = format!("csv: +{added} ~{merged} -{deleted}");
    if soft > 0 {
        let _ = write!(note, " ({soft} soft)");
    }
    note
}

/// Join two note segments with `; `, dropping either when empty.
#[must_use]
pub fn join_notes(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_owned(),
        (_, true) => a.to_owned(),
        _ => format!("{a}; {b}"),
    }
}

/// Whether an interrupted/missing baseline should trigger one automatic resync.
///
/// True only when the run aborted with `PriorListingMissing` AND this wasn't
/// already a resync (a resync re-establishes the baseline, so never loop on it).
#[must_use]
pub fn should_auto_resync(dir: Direction, abort: Option<&crate::sync::run::AbortKind>) -> bool {
    dir != Direction::Resync
        && matches!(
            abort,
            Some(crate::sync::run::AbortKind::PriorListingMissing)
        )
}

/// Whether a normal sync should automatically run the narrow, low-risk repair
/// for a missing check-access marker. A resync never retries itself.
#[must_use]
pub fn should_auto_repair_check_access(
    dir: Direction,
    abort: Option<&crate::sync::run::AbortKind>,
) -> bool {
    dir != Direction::Resync && matches!(abort, Some(crate::sync::run::AbortKind::CheckAccess))
}

/// Whether this sync run should create/repair the check-access markers before
/// invoking rclone.
#[must_use]
pub fn should_bootstrap_check_access(dir: Direction) -> bool {
    dir == Direction::Resync
}

#[must_use]
pub fn format_sync_plan(cfg: &SyncConfig, root: &Path, dir: Direction, theme: Theme) -> String {
    let remote = build_remote(cfg);
    format_sync_plan_for_remote(root, &remote, dir, theme)
}

#[must_use]
pub fn format_sync_plan_for_remote(
    root: &Path,
    remote: &Remote,
    dir: Direction,
    theme: Theme,
) -> String {
    let heading = match dir {
        Direction::Both => "Syncing brain",
        Direction::Push => "Pushing local brain changes",
        Direction::Pull => "Pulling remote brain changes",
        Direction::Resync => "Repairing cloud sync metadata",
    };
    format!(
        "{}\n  {} {}\n  {} {}",
        theme.heading(heading),
        theme.muted("local:"),
        theme.value(&root.display().to_string()),
        theme.muted("remote:"),
        theme.value(&remote.arg),
    )
}

#[must_use]
pub fn sync_progress(dir: Direction) -> &'static str {
    match dir {
        Direction::Both => "Comparing local and remote changes, then syncing both directions…",
        Direction::Push => "Comparing local and remote changes, then pushing local changes…",
        Direction::Pull => "Comparing local and remote changes, then pulling remote changes…",
        Direction::Resync => "Checking the sync marker and rebuilding the rclone baseline…",
    }
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

/// Map the `--push`/`--pull` flags to a `Direction` for a bare `brain sync`.
pub fn direction_from_flags(push: bool, pull: bool) -> Result<Direction> {
    match (push, pull) {
        (true, true) => bail!("--push and --pull are mutually exclusive"),
        (true, false) => Ok(Direction::Push),
        (false, true) => Ok(Direction::Pull),
        (false, false) => Ok(Direction::Both),
    }
}

/// User-facing guidance for sync commands run before `brain sync setup`.
#[must_use]
pub fn format_unconfigured_sync_guidance(dir: Direction, theme: Theme) -> String {
    let setup = theme.accent("brain sync setup");
    if dir == Direction::Resync {
        return format!(
            "{}\n\n`{}` only repairs an existing sync setup: it recreates the RCLONE_TEST marker and re-establishes the rclone baseline. It does not collect Backblaze credentials or enable cloud sync.\n\nRun `{setup}`.",
            theme.warning("Cloud sync is not set up yet."),
            theme.accent("brain sync repair"),
        );
    }
    format!(
        "{}\n\nRun `{setup}` to connect a private Backblaze B2 bucket, save this machine's sync credentials, create the RCLONE_TEST marker, and establish the first baseline.",
        theme.warning("Cloud sync is not set up yet."),
    )
}

/// Format the "a sync is running right now" status line (pure).
#[must_use]
pub fn format_in_progress(state: &crate::sync::current::CurrentState, theme: Theme) -> String {
    format!(
        "{} {} · started {} · {}",
        theme.info("syncing now:"),
        theme.accent(&state.direction),
        theme.value(&state.started_at),
        theme.muted(&format!("pid {}", state.pid)),
    )
}

/// Format the status line for the most recent journal run (pure).
#[must_use]
pub fn format_last_run(run: Option<&SyncRun>, theme: Theme) -> String {
    run.map_or_else(
        || "no syncs yet — run `brain sync`.".to_owned(),
        |r| {
            let outcome = match r.outcome.as_str() {
                "clean" => theme.success(&r.outcome),
                "needs_attention" => theme.warning(&r.outcome),
                "aborted" => theme.error(&r.outcome),
                _ => r.outcome.clone(),
            };
            format!(
                "last sync: {} · {} · {} · {}↑ {}↓ {} conflicts{}",
                theme.muted(&r.finished_at),
                theme.accent(&r.direction),
                outcome,
                theme.accent(&r.transferred.to_string()),
                theme.accent(&r.deleted.to_string()),
                theme.accent(&r.conflicts.to_string()),
                if r.note.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", theme.muted(&r.note))
                },
            )
        },
    )
}

/// Format the configured auto-sync triggers. The flags are honored once the
/// trigger/watcher phase lands; `status` shows them so the setup is visible.
#[must_use]
pub fn format_triggers(cfg: &SyncConfig, theme: Theme) -> String {
    let yn = |b: bool| {
        if b {
            theme.success("on")
        } else {
            theme.muted("off")
        }
    };
    let watch_on = cfg.watch_effective();
    let debounce = if watch_on {
        format!(
            " {}",
            theme.muted(&format!("({}ms debounce)", cfg.debounce_ms))
        )
    } else {
        String::new()
    };
    let idle = cfg.idle_pull_interval().map_or_else(
        || theme.muted("off"),
        |interval| theme.success(&format!("{}s", interval.as_secs())),
    );
    format!(
        "{} on-start {} · on-exit {} · watch {}{} · idle-pull {}",
        theme.muted("triggers:"),
        yn(cfg.on_start),
        yn(cfg.on_exit),
        yn(watch_on),
        debounce,
        idle,
    )
}

/// Print `brain sync status`.
pub fn print_status(cfg: &SyncConfig, root: &Path) -> Result<()> {
    let theme = Theme::active();
    if !cfg.is_configured() {
        crate::logging::log("sync status unconfigured");
        println!(
            "{}",
            format_unconfigured_sync_guidance(Direction::Both, theme)
        );
        return Ok(());
    }
    crate::logging::log(format!(
        "sync status journal={} root={}",
        Journal::default_path().display(),
        root.display()
    ));
    // Surface a sync happening right now (in a detached background process or
    // another shell) above the last completed run, so status always answers
    // "is anything syncing?" first.
    if let Some(state) = crate::sync::current::read_state() {
        if crate::server::lifecycle::pid_alive(state.pid) {
            crate::logging::log("sync status in-progress");
            println!("{}", format_in_progress(&state, theme));
        }
    }
    let journal = Journal::open(&Journal::default_path())?;
    let recent = journal.recent(1)?;
    println!("{}", format_last_run(recent.first(), theme));
    println!("{}", format_triggers(cfg, theme));
    let conflicts = conflicts::list_conflicts(root);
    let count = conflicts.len();
    crate::logging::log(format!("sync status conflicts={count}"));
    let label = if count > 0 {
        theme.warning("open conflicts:")
    } else {
        theme.muted("open conflicts:")
    };
    println!("{} {}", label, theme.accent(&count.to_string()));
    Ok(())
}

/// Per-copy filesystem metadata, injected so [`conflicts_json`] stays pure.
pub struct CopyMeta {
    pub modified: Option<String>,
    pub bytes: Option<u64>,
}

/// Build the `brain sync conflicts --json` value. Pure.
///
/// `meta(rel_path)` supplies each copy's metadata (`None` fields serialize to
/// JSON `null`); `exists(original)` says whether the canonical file is
/// present. An empty `groups` slice builds an empty JSON array.
#[must_use]
pub fn conflicts_json(
    groups: &[ConflictGroup],
    meta: impl Fn(&Path) -> CopyMeta,
    exists: impl Fn(&Path) -> bool,
) -> serde_json::Value {
    let value: Vec<serde_json::Value> = groups
        .iter()
        .map(|g| {
            let copies: Vec<serde_json::Value> = g
                .copies
                .iter()
                .map(|c| {
                    let m = meta(&c.path);
                    serde_json::json!({
                        "path": c.path.display().to_string(),
                        "host": c.host,
                        "date": c.date,
                        "modified": m.modified,
                        "bytes": m.bytes,
                    })
                })
                .collect();
            serde_json::json!({
                "original": g.original.display().to_string(),
                "original_exists": exists(&g.original),
                "copies": copies,
            })
        })
        .collect();
    serde_json::Value::Array(value)
}

/// Read a copy's mtime/size off disk for [`conflicts_json`]; missing file or
/// unreadable mtime degrades to `None` rather than failing the whole command.
fn copy_meta_from_fs(root: &Path, rel: &Path) -> CopyMeta {
    let Ok(m) = fs::metadata(root.join(rel)) else {
        return CopyMeta {
            modified: None,
            bytes: None,
        };
    };
    let modified = m.modified().ok().map(|t| {
        DateTime::<Utc>::from(t)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    });
    CopyMeta {
        modified,
        bytes: Some(m.len()),
    }
}

/// Human-list paths for `brain sync conflicts`.
///
/// Built from the same strict grouping parser as `--json` so both surfaces
/// agree on what is a real friendly conflict copy.
#[must_use]
pub fn conflict_display_paths(files: &[conflicts::ConflictFile]) -> Vec<std::path::PathBuf> {
    conflicts::group_conflicts(files)
        .into_iter()
        .flat_map(|group| group.copies.into_iter().map(|copy| copy.path))
        .collect()
}

/// Print `brain sync conflicts`. `json == true` emits the structured
/// `conflicts_json` shape to stdout; otherwise this is the themed human list
/// rendered through the same strict grouping path.
pub fn print_conflicts(root: &Path, json: bool) -> Result<()> {
    let theme = Theme::active();
    crate::logging::log(format!(
        "sync conflicts scan root={} json={json}",
        root.display()
    ));
    let conflicts = conflicts::list_conflicts(root);
    crate::logging::log(format!("sync conflicts raw_copies={}", conflicts.len()));
    if json {
        let groups = conflicts::group_conflicts(&conflicts);
        let meta = |rel: &Path| copy_meta_from_fs(root, rel);
        let exists = |rel: &Path| root.join(rel).exists();
        let value = conflicts_json(&groups, meta, exists);
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        let display_paths = conflict_display_paths(&conflicts);
        if display_paths.is_empty() {
            println!("{}", theme.muted("no open conflict copies."));
        } else {
            for path in display_paths {
                println!("{}", theme.value(&path.display().to_string()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::sync::conflicts::{ConflictGroup, ParsedCopy};
    use crate::theme::Theme;

    #[test]
    fn conflicts_json_empty_groups_is_empty_array() {
        let groups: Vec<ConflictGroup> = vec![];
        let meta = |_: &Path| CopyMeta {
            modified: None,
            bytes: None,
        };
        let exists = |_: &Path| false;
        let v = conflicts_json(&groups, meta, exists);
        assert_eq!(v, serde_json::json!([]));
    }

    #[test]
    fn conflicts_json_builds_group_with_injected_copy_metadata() {
        let groups = vec![ConflictGroup {
            original: PathBuf::from("resources/ai/idea.md"),
            copies: vec![ParsedCopy {
                path: PathBuf::from("resources/ai/idea (conflict mac 2026-07-25).md"),
                host: "mac".to_owned(),
                date: "2026-07-25".to_owned(),
            }],
        }];
        let meta = |_: &Path| CopyMeta {
            modified: Some("2026-07-25T10:04:11Z".to_owned()),
            bytes: Some(1841),
        };
        let exists = |_: &Path| true;
        let v = conflicts_json(&groups, meta, exists);
        assert_eq!(
            v,
            serde_json::json!([{
                "original": "resources/ai/idea.md",
                "original_exists": true,
                "copies": [{
                    "path": "resources/ai/idea (conflict mac 2026-07-25).md",
                    "host": "mac",
                    "date": "2026-07-25",
                    "modified": "2026-07-25T10:04:11Z",
                    "bytes": 1841
                }]
            }])
        );
    }

    #[test]
    fn conflicts_json_missing_metadata_serializes_as_null_not_omitted() {
        let groups = vec![ConflictGroup {
            original: PathBuf::from("notes.md"),
            copies: vec![ParsedCopy {
                path: PathBuf::from("notes (conflict mac 2026-07-25).md"),
                host: "mac".to_owned(),
                date: "2026-07-25".to_owned(),
            }],
        }];
        let meta = |_: &Path| CopyMeta {
            modified: None,
            bytes: None,
        };
        let exists = |_: &Path| false;
        let v = conflicts_json(&groups, meta, exists);
        let copy = &v[0]["copies"][0];
        assert!(copy["modified"].is_null(), "{v}");
        assert!(copy["bytes"].is_null(), "{v}");
        assert_eq!(v[0]["original_exists"], false);
    }

    #[test]
    fn conflicts_json_missing_fs_metadata_serializes_as_null() {
        let tmp = std::env::temp_dir().join(format!("brain-conflict-meta-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let groups = vec![ConflictGroup {
            original: PathBuf::from("notes.md"),
            copies: vec![ParsedCopy {
                path: PathBuf::from("notes (conflict mac 2026-07-25).md"),
                host: "mac".to_owned(),
                date: "2026-07-25".to_owned(),
            }],
        }];

        let v = conflicts_json(
            &groups,
            |rel| copy_meta_from_fs(&tmp, rel),
            |rel| tmp.join(rel).exists(),
        );

        let copy = &v[0]["copies"][0];
        assert!(copy["modified"].is_null(), "{v}");
        assert!(copy["bytes"].is_null(), "{v}");
        assert_eq!(v[0]["original_exists"], false);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn conflict_display_paths_drop_loose_non_parseable_matches() {
        let files = vec![
            crate::sync::conflicts::ConflictFile {
                path: PathBuf::from("idea (conflict mac 2026-07-25).md"),
            },
            crate::sync::conflicts::ConflictFile {
                path: PathBuf::from("not actually (conflict text).md"),
            },
        ];

        assert_eq!(
            conflict_display_paths(&files),
            vec![PathBuf::from("idea (conflict mac 2026-07-25).md")]
        );
    }

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
    fn format_in_progress_names_the_running_direction_and_start() {
        let state = crate::sync::current::CurrentState {
            pid: 4242,
            direction: "both".into(),
            started_at: "2026-07-29T01:00:00Z".into(),
        };
        let line = format_in_progress(&state, Theme::dark(false));
        assert!(line.contains("syncing now"), "{line}");
        assert!(line.contains("both"), "{line}");
        assert!(line.contains("2026-07-29T01:00:00Z"), "{line}");
        assert!(line.contains("pid 4242"), "{line}");
    }

    #[test]
    fn format_last_run_handles_empty_and_populated() {
        let theme = Theme::dark(false);
        assert!(format_last_run(None, theme).contains("no syncs yet"));
        let r = crate::sync::journal::SyncRun {
            started_at: "s".into(),
            finished_at: "2026-07-25T00:00:05Z".into(),
            direction: "both".into(),
            outcome: "clean".into(),
            transferred: 3,
            deleted: 1,
            conflicts: 0,
            errors: 0,
            note: String::new(),
        };
        let line = format_last_run(Some(&r), theme);
        assert!(line.contains("both") && line.contains("clean") && line.contains("3↑"));
    }

    #[test]
    fn format_last_run_colors_the_outcome_by_value() {
        let clean_run = crate::sync::journal::SyncRun {
            started_at: "s".into(),
            finished_at: "2026-07-25T00:00:05Z".into(),
            direction: "both".into(),
            outcome: "clean".into(),
            transferred: 3,
            deleted: 1,
            conflicts: 0,
            errors: 0,
            note: String::new(),
        };
        let line = format_last_run(Some(&clean_run), Theme::dark(true));
        assert!(
            line.contains("\x1b[92m"),
            "clean outcome should be colored success green: {line}"
        );

        let aborted_run = crate::sync::journal::SyncRun {
            outcome: "aborted".into(),
            ..clean_run
        };
        let line = format_last_run(Some(&aborted_run), Theme::dark(true));
        assert!(
            line.contains("\x1b[91m"),
            "aborted outcome should be colored error red: {line}"
        );
    }

    #[test]
    fn format_triggers_reads_the_configured_flags() {
        let cfg: SyncConfig =
            serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","on_start":false}"#).unwrap();
        let s = format_triggers(&cfg, Theme::dark(false));
        assert!(s.contains("on-start off"), "{s}");
        assert!(s.contains("on-exit on"), "{s}"); // default true
        assert!(s.contains("watch on"), "{s}"); // configured + default watch
        assert!(s.contains("idle-pull off"), "{s}"); // default opt-out
    }

    #[test]
    fn format_triggers_shows_debounce_window_when_watch_on() {
        let cfg: SyncConfig = serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b"}"#).unwrap();
        let line = format_triggers(&cfg, Theme::dark(false));
        assert!(line.contains("watch on"), "{line}");
        assert!(line.contains("3000ms"), "{line}");
    }

    #[test]
    fn format_triggers_hides_debounce_window_when_watch_off() {
        // watch is disabled → the debounce window is meaningless, so don't show it.
        let cfg: SyncConfig =
            serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","watch":false}"#).unwrap();
        let line = format_triggers(&cfg, Theme::dark(false));
        assert!(line.contains("watch off"), "{line}");
        assert!(!line.contains("debounce"), "{line}");
    }

    #[test]
    fn format_triggers_shows_idle_pull_interval_when_enabled() {
        let cfg: SyncConfig =
            serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","idle_pull_secs":120}"#)
                .unwrap();
        let line = format_triggers(&cfg, Theme::dark(false));
        assert!(line.contains("idle-pull 120s"), "{line}");
    }

    #[test]
    fn format_triggers_colors_on_and_off_flags() {
        let cfg: SyncConfig =
            serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","on_start":false}"#).unwrap();
        let s = format_triggers(&cfg, Theme::dark(true));
        assert!(
            s.contains("\x1b[92m"),
            "on flags should be success green: {s}"
        );
        assert!(
            s.contains("\x1b[90m"),
            "off flags should be muted gray: {s}"
        );
    }

    #[test]
    fn flags_map_to_direction() {
        assert_eq!(direction_from_flags(false, false).unwrap(), Direction::Both);
        assert_eq!(direction_from_flags(true, false).unwrap(), Direction::Push);
        assert_eq!(direction_from_flags(false, true).unwrap(), Direction::Pull);
        assert!(direction_from_flags(true, true).is_err());
    }

    #[test]
    fn auto_resyncs_only_on_prior_listing_missing_and_not_already_a_resync() {
        use crate::sync::run::AbortKind;
        assert!(should_auto_resync(
            Direction::Both,
            Some(&AbortKind::PriorListingMissing)
        ));
        assert!(should_auto_resync(
            Direction::Push,
            Some(&AbortKind::PriorListingMissing)
        ));
        // already a resync -> don't loop
        assert!(!should_auto_resync(
            Direction::Resync,
            Some(&AbortKind::PriorListingMissing)
        ));
        // other aborts / clean -> no auto resync
        assert!(!should_auto_resync(
            Direction::Both,
            Some(&AbortKind::MaxDelete)
        ));
        assert!(!should_auto_resync(Direction::Both, None));
    }

    #[test]
    fn check_access_abort_is_auto_repaired_once_for_normal_syncs() {
        use crate::sync::run::AbortKind;
        assert!(should_auto_repair_check_access(
            Direction::Both,
            Some(&AbortKind::CheckAccess)
        ));
        assert!(should_auto_repair_check_access(
            Direction::Push,
            Some(&AbortKind::CheckAccess)
        ));
        assert!(!should_auto_repair_check_access(
            Direction::Resync,
            Some(&AbortKind::CheckAccess)
        ));
        assert!(!should_auto_repair_check_access(
            Direction::Both,
            Some(&AbortKind::PriorListingMissing)
        ));
    }

    #[test]
    fn check_access_bootstrap_runs_only_for_resync() {
        assert!(should_bootstrap_check_access(Direction::Resync));
        assert!(!should_bootstrap_check_access(Direction::Both));
        assert!(!should_bootstrap_check_access(Direction::Push));
        assert!(!should_bootstrap_check_access(Direction::Pull));
    }

    #[test]
    fn sync_plan_for_repair_names_each_slow_phase_up_front() {
        let cfg: SyncConfig =
            serde_json::from_str(r#"{"enabled":true,"b2_bucket":"bucket","b2_path":"brain-root"}"#)
                .unwrap();
        let plan = format_sync_plan(
            &cfg,
            Path::new("/tmp/brain"),
            Direction::Resync,
            Theme::dark(false),
        );

        assert!(plan.contains("Repairing cloud sync metadata"), "{plan}");
        assert!(plan.contains("local: /tmp/brain"), "{plan}");
        assert!(plan.contains("remote: BRAIN:bucket/brain-root"), "{plan}");
        assert!(!plan.contains("plan:"), "{plan}");
        assert!(!plan.contains("then:"), "{plan}");
    }

    #[test]
    fn sync_progress_describes_each_direction_without_a_plan_block() {
        assert_eq!(
            sync_progress(Direction::Both),
            "Comparing local and remote changes, then syncing both directions…"
        );
        assert_eq!(
            sync_progress(Direction::Push),
            "Comparing local and remote changes, then pushing local changes…"
        );
    }

    #[test]
    fn missing_rclone_guidance_names_both_install_commands() {
        let message = crate::sync::run::missing_rclone_guidance(Theme::dark(false), "brain sync");
        assert!(message.contains("rclone is not installed"), "{message}");
        assert!(
            message.contains("If you have Homebrew installed, use this option:"),
            "{message}"
        );
        assert!(
            message.contains("If you do not have Homebrew, use this option:"),
            "{message}"
        );
        assert!(message.contains("brew install rclone"), "{message}");
        assert!(
            message.contains("sudo -v ; curl https://rclone.org/install.sh | sudo bash"),
            "{message}"
        );
    }

    #[test]
    fn csv_note_is_empty_when_nothing_changed() {
        assert_eq!(format_csv_note(&[]), "");
        assert_eq!(
            format_csv_note(&[crate::sync::csv_sync::CsvMergeOutcome::default()]),
            ""
        );
    }

    #[test]
    fn csv_note_sums_added_merged_deleted_and_flags_soft_conflicts() {
        use crate::sync::csv_sync::CsvMergeOutcome;
        let outcomes = [
            CsvMergeOutcome {
                name: "tasks.csv".into(),
                added: 2,
                deleted: 1,
                merged: 3,
                soft_conflicts: 1,
            },
            CsvMergeOutcome {
                name: "habits.csv".into(),
                added: 1,
                deleted: 0,
                merged: 0,
                soft_conflicts: 0,
            },
        ];
        assert_eq!(format_csv_note(&outcomes), "csv: +3 ~3 -1 (1 soft)");
    }

    #[test]
    fn csv_note_omits_soft_suffix_when_none() {
        use crate::sync::csv_sync::CsvMergeOutcome;
        let outcomes = [CsvMergeOutcome {
            name: "tasks.csv".into(),
            added: 1,
            ..Default::default()
        }];
        assert_eq!(format_csv_note(&outcomes), "csv: +1 ~0 -0");
    }

    #[test]
    fn sync_once_refuses_when_unconfigured() {
        let cfg: SyncConfig = serde_json::from_str("{}").unwrap();
        let err = sync_once(
            &cfg,
            Path::new("/tmp"),
            Direction::Both,
            ("a", "b", "2026-07-25"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("brain sync setup"));
    }

    #[test]
    fn sync_repair_before_setup_points_to_setup() {
        let message = format_unconfigured_sync_guidance(Direction::Resync, Theme::dark(false));

        assert!(
            message.contains("Cloud sync is not set up yet."),
            "{message}"
        );
        assert!(
            message.contains("`brain sync repair` only repairs an existing sync setup"),
            "{message}"
        );
        assert!(message.contains("Run `brain sync setup`."), "{message}");
    }
}
