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
use crate::sync::remote::build_remote;
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
pub fn sync_once(cfg: &SyncConfig, root: &Path, dir: Direction, now: (&str, &str, &str)) -> Result<Outcome> {
    if !cfg.is_configured() {
        bail!("sync is not configured — run `brain sync setup`");
    }
    let (started_at, finished_at, date) = now;
    let remote = build_remote(cfg);
    let local = root.to_string_lossy().into_owned();
    let argv = args::bisync_args(cfg, &local, &remote.arg, dir);

    if should_bootstrap_check_access(dir) {
        crate::sync::check_access::ensure_markers(root, &remote)?;
    }

    let mut run = run_rclone(&remote.env, &argv);
    let resumed = if should_auto_resync(dir, run.abort.as_ref()) {
        let theme = Theme::active();
        eprintln!(
            "{}",
            theme.warning("Baseline was incomplete (a prior sync was interrupted); resuming with a resync…")
        );
        let resync_argv = args::bisync_args(cfg, &local, &remote.arg, Direction::Resync);
        run = run_rclone(&remote.env, &resync_argv);
        true
    } else {
        false
    };
    let renamed_count = conflicts::rename_markers(root, &hostname(), date);
    let renamed = u64::try_from(renamed_count).unwrap_or(0);
    let leftover = conflicts::leftover_markers(root);
    let outcome = verify::classify(&run, renamed_count, leftover);

    // The two task CSVs are excluded from bisync and reconciled out-of-band via
    // the 3-way merge. Best-effort: skip on an abort, and never let a CSV
    // failure change the bisync outcome — just record what merged.
    let csv_note = if matches!(outcome, Outcome::Aborted(_)) {
        String::new()
    } else {
        format_csv_note(&crate::sync::csv_sync::sync_csvs(cfg, root))
    };

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
        note: {
            let base = match &outcome {
                Outcome::Clean => String::new(),
                Outcome::NeedsAttention(m) | Outcome::Aborted(m) => m.clone(),
            };
            let base = if resumed {
                if base.is_empty() { "auto-resumed after interrupted baseline".to_owned() }
                else { format!("auto-resumed after interrupted baseline; {base}") }
            } else {
                base
            };
            join_notes(&base, &csv_note)
        },
    })?;
    Ok(outcome)
}

/// Summarize the CSV merge outcomes into a journal note segment, e.g.
/// `csv: +3 ~2 -1 (1 soft)`. Empty when nothing was added, merged, deleted, or
/// soft-conflicted, so a clean run stays noise-free.
#[must_use]
pub fn format_csv_note(outcomes: &[CsvMergeOutcome]) -> String {
    let (added, merged, deleted, soft) = outcomes.iter().fold((0, 0, 0, 0), |acc, o| {
        (acc.0 + o.added, acc.1 + o.merged, acc.2 + o.deleted, acc.3 + o.soft_conflicts)
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
        && matches!(abort, Some(crate::sync::run::AbortKind::PriorListingMissing))
}

/// Whether this sync run should create/repair the check-access markers before
/// invoking rclone.
#[must_use]
pub fn should_bootstrap_check_access(dir: Direction) -> bool {
    dir == Direction::Resync
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
                if r.note.is_empty() { String::new() } else { format!(" · {}", theme.muted(&r.note)) },
            )
        },
    )
}

/// Format the configured auto-sync triggers. The flags are honored once the
/// trigger/watcher phase lands; `status` shows them so the setup is visible.
#[must_use]
pub fn format_triggers(cfg: &SyncConfig, theme: Theme) -> String {
    let yn = |b: bool| if b { theme.success("on") } else { theme.muted("off") };
    let watch_on = cfg.watch_effective();
    let debounce = if watch_on {
        format!(" {}", theme.muted(&format!("({}ms debounce)", cfg.debounce_ms)))
    } else {
        String::new()
    };
    format!(
        "{} on-start {} · on-exit {} · watch {}{}",
        theme.muted("triggers:"),
        yn(cfg.on_start),
        yn(cfg.on_exit),
        yn(watch_on),
        debounce,
    )
}

/// Print `brain sync status`.
pub fn print_status(cfg: &SyncConfig, root: &Path) -> Result<()> {
    let theme = Theme::active();
    if !cfg.is_configured() {
        println!(
            "{} run `{}`.",
            theme.warning("sync is not configured —"),
            theme.accent("brain sync setup")
        );
        return Ok(());
    }
    let journal = Journal::open(&Journal::default_path())?;
    let recent = journal.recent(1)?;
    println!("{}", format_last_run(recent.first(), theme));
    println!("{}", format_triggers(cfg, theme));
    let conflicts = conflicts::list_conflicts(root);
    let count = conflicts.len();
    let label = if count > 0 { theme.warning("open conflicts:") } else { theme.muted("open conflicts:") };
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
        return CopyMeta { modified: None, bytes: None };
    };
    let modified =
        m.modified().ok().map(|t| DateTime::<Utc>::from(t).format("%Y-%m-%dT%H:%M:%SZ").to_string());
    CopyMeta { modified, bytes: Some(m.len()) }
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
    let conflicts = conflicts::list_conflicts(root);
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
        let meta = |_: &Path| CopyMeta { modified: None, bytes: None };
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
        let meta = |_: &Path| CopyMeta { modified: Some("2026-07-25T10:04:11Z".to_owned()), bytes: Some(1841) };
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
        let meta = |_: &Path| CopyMeta { modified: None, bytes: None };
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
    fn format_last_run_handles_empty_and_populated() {
        let theme = Theme::dark(false);
        assert!(format_last_run(None, theme).contains("no syncs yet"));
        let r = crate::sync::journal::SyncRun {
            started_at: "s".into(), finished_at: "2026-07-25T00:00:05Z".into(),
            direction: "both".into(), outcome: "clean".into(),
            transferred: 3, deleted: 1, conflicts: 0, errors: 0, note: String::new(),
        };
        let line = format_last_run(Some(&r), theme);
        assert!(line.contains("both") && line.contains("clean") && line.contains("3↑"));
    }

    #[test]
    fn format_last_run_colors_the_outcome_by_value() {
        let clean_run = crate::sync::journal::SyncRun {
            started_at: "s".into(), finished_at: "2026-07-25T00:00:05Z".into(),
            direction: "both".into(), outcome: "clean".into(),
            transferred: 3, deleted: 1, conflicts: 0, errors: 0, note: String::new(),
        };
        let line = format_last_run(Some(&clean_run), Theme::dark(true));
        assert!(line.contains("\x1b[92m"), "clean outcome should be colored success green: {line}");

        let aborted_run = crate::sync::journal::SyncRun { outcome: "aborted".into(), ..clean_run };
        let line = format_last_run(Some(&aborted_run), Theme::dark(true));
        assert!(line.contains("\x1b[91m"), "aborted outcome should be colored error red: {line}");
    }

    #[test]
    fn format_triggers_reads_the_configured_flags() {
        let cfg: SyncConfig =
            serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","on_start":false}"#).unwrap();
        let s = format_triggers(&cfg, Theme::dark(false));
        assert!(s.contains("on-start off"), "{s}");
        assert!(s.contains("on-exit on"), "{s}"); // default true
        assert!(s.contains("watch on"), "{s}"); // configured + default watch
    }

    #[test]
    fn format_triggers_shows_debounce_window_when_watch_on() {
        let cfg: SyncConfig =
            serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b"}"#).unwrap();
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
    fn format_triggers_colors_on_and_off_flags() {
        let cfg: SyncConfig =
            serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","on_start":false}"#).unwrap();
        let s = format_triggers(&cfg, Theme::dark(true));
        assert!(s.contains("\x1b[92m"), "on flags should be success green: {s}");
        assert!(s.contains("\x1b[90m"), "off flags should be muted gray: {s}");
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
        assert!(should_auto_resync(Direction::Both, Some(&AbortKind::PriorListingMissing)));
        assert!(should_auto_resync(Direction::Push, Some(&AbortKind::PriorListingMissing)));
        // already a resync -> don't loop
        assert!(!should_auto_resync(Direction::Resync, Some(&AbortKind::PriorListingMissing)));
        // other aborts / clean -> no auto resync
        assert!(!should_auto_resync(Direction::Both, Some(&AbortKind::MaxDelete)));
        assert!(!should_auto_resync(Direction::Both, None));
    }

    #[test]
    fn check_access_bootstrap_runs_only_for_resync() {
        assert!(should_bootstrap_check_access(Direction::Resync));
        assert!(!should_bootstrap_check_access(Direction::Both));
        assert!(!should_bootstrap_check_access(Direction::Push));
        assert!(!should_bootstrap_check_access(Direction::Pull));
    }

    #[test]
    fn csv_note_is_empty_when_nothing_changed() {
        assert_eq!(format_csv_note(&[]), "");
        assert_eq!(format_csv_note(&[crate::sync::csv_sync::CsvMergeOutcome::default()]), "");
    }

    #[test]
    fn csv_note_sums_added_merged_deleted_and_flags_soft_conflicts() {
        use crate::sync::csv_sync::CsvMergeOutcome;
        let outcomes = [
            CsvMergeOutcome { name: "tasks.csv".into(), added: 2, deleted: 1, merged: 3, soft_conflicts: 1 },
            CsvMergeOutcome { name: "habits.csv".into(), added: 1, deleted: 0, merged: 0, soft_conflicts: 0 },
        ];
        assert_eq!(format_csv_note(&outcomes), "csv: +3 ~3 -1 (1 soft)");
    }

    #[test]
    fn csv_note_omits_soft_suffix_when_none() {
        use crate::sync::csv_sync::CsvMergeOutcome;
        let outcomes = [CsvMergeOutcome { name: "tasks.csv".into(), added: 1, ..Default::default() }];
        assert_eq!(format_csv_note(&outcomes), "csv: +1 ~0 -0");
    }

    #[test]
    fn sync_once_refuses_when_unconfigured() {
        let cfg: SyncConfig = serde_json::from_str("{}").unwrap();
        let err = sync_once(&cfg, Path::new("/tmp"), Direction::Both, ("a", "b", "2026-07-25")).unwrap_err();
        assert!(err.to_string().contains("brain sync setup"));
    }
}
