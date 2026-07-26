//! `brain sync` orchestration.
//!
//! Ties the pure builders to the rclone shell, the conflict post-pass,
//! verification, and the journal. Kept thin; the tested logic lives in the
//! builders it calls.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Result, bail};

use crate::sync::args::{self, Direction};
use crate::sync::config::SyncConfig;
use crate::sync::conflicts;
use crate::sync::csv_sync::CsvMergeOutcome;
use crate::sync::journal::{Journal, SyncRun};
use crate::sync::remote::build_remote;
use crate::sync::run::run_rclone;
use crate::sync::verify::{self, Outcome};
use crate::theme::Theme;

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
    format!(
        "{} on-start {} · on-exit {} · watch {} {}",
        theme.muted("triggers:"),
        yn(cfg.on_start),
        yn(cfg.on_exit),
        yn(cfg.watch_effective()),
        theme.muted(&format!("({}ms debounce)", cfg.debounce_ms)),
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

/// Print `brain sync conflicts`.
pub fn print_conflicts(root: &Path) {
    let theme = Theme::active();
    let conflicts = conflicts::list_conflicts(root);
    if conflicts.is_empty() {
        println!("{}", theme.muted("no open conflict copies."));
    } else {
        for c in conflicts {
            println!("{}", theme.value(&c.path.display().to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

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
