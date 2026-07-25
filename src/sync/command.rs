//! `brain sync` orchestration.
//!
//! Ties the pure builders to the rclone shell, the conflict post-pass,
//! verification, and the journal. Kept thin; the tested logic lives in the
//! builders it calls.

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

    let mut run = run_rclone(&remote.env, &argv);
    let resumed = if should_auto_resync(dir, run.abort.as_ref()) {
        eprintln!("Baseline was incomplete (a prior sync was interrupted); resuming with a resync…");
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
            if resumed {
                if base.is_empty() { "auto-resumed after interrupted baseline".to_owned() }
                else { format!("auto-resumed after interrupted baseline; {base}") }
            } else {
                base
            }
        },
    })?;
    Ok(outcome)
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
pub fn format_last_run(run: Option<&SyncRun>) -> String {
    run.map_or_else(
        || "no syncs yet — run `brain sync`.".to_owned(),
        |r| {
            format!(
                "last sync: {} · {} · {} · {}↑ {}↓ {} conflicts{}",
                r.finished_at, r.direction, r.outcome, r.transferred, r.deleted, r.conflicts,
                if r.note.is_empty() { String::new() } else { format!(" · {}", r.note) },
            )
        },
    )
}

/// Format the configured auto-sync triggers. The flags are honored once the
/// trigger/watcher phase lands; `status` shows them so the setup is visible.
#[must_use]
pub fn format_triggers(cfg: &SyncConfig) -> String {
    let yn = |b: bool| if b { "on" } else { "off" };
    format!(
        "triggers: on-start {} · on-exit {} · watch {}",
        yn(cfg.on_start),
        yn(cfg.on_exit),
        yn(cfg.watch_effective()),
    )
}

/// Print `brain sync status`.
pub fn print_status(cfg: &SyncConfig, root: &Path) -> Result<()> {
    if !cfg.is_configured() {
        println!("sync is not configured — run `brain sync setup`.");
        return Ok(());
    }
    let journal = Journal::open(&Journal::default_path())?;
    let recent = journal.recent(1)?;
    println!("{}", format_last_run(recent.first()));
    println!("{}", format_triggers(cfg));
    let conflicts = conflicts::list_conflicts(root);
    println!("open conflicts: {}", conflicts.len());
    Ok(())
}

/// Print `brain sync conflicts`.
pub fn print_conflicts(root: &Path) {
    let conflicts = conflicts::list_conflicts(root);
    if conflicts.is_empty() {
        println!("no open conflict copies.");
    } else {
        for c in conflicts {
            println!("{}", c.path.display());
        }
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

    #[test]
    fn format_triggers_reads_the_configured_flags() {
        let cfg: SyncConfig =
            serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b","on_start":false}"#).unwrap();
        let s = format_triggers(&cfg);
        assert!(s.contains("on-start off"), "{s}");
        assert!(s.contains("on-exit on"), "{s}"); // default true
        assert!(s.contains("watch on"), "{s}"); // configured + default watch
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
    fn sync_once_refuses_when_unconfigured() {
        let cfg: SyncConfig = serde_json::from_str("{}").unwrap();
        let err = sync_once(&cfg, Path::new("/tmp"), Direction::Both, ("a", "b", "2026-07-25")).unwrap_err();
        assert!(err.to_string().contains("brain sync setup"));
    }
}
