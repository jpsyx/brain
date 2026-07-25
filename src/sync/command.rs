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

    let run = run_rclone(&remote.env, &argv);
    let renamed = u64::try_from(conflicts::rename_markers(root, &hostname(), date)).unwrap_or(0);
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
    fn flags_map_to_direction() {
        assert_eq!(direction_from_flags(false, false).unwrap(), Direction::Both);
        assert_eq!(direction_from_flags(true, false).unwrap(), Direction::Push);
        assert_eq!(direction_from_flags(false, true).unwrap(), Direction::Pull);
        assert!(direction_from_flags(true, true).is_err());
    }

    #[test]
    fn sync_once_refuses_when_unconfigured() {
        let cfg: SyncConfig = serde_json::from_str("{}").unwrap();
        let err = sync_once(&cfg, Path::new("/tmp"), Direction::Both, ("a", "b", "2026-07-25")).unwrap_err();
        assert!(err.to_string().contains("brain sync setup"));
    }
}
