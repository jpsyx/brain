//! The *in-flight* sync's shared state, so a sync that runs in a detached
//! background process is still observable from any other `brain` invocation.
//!
//! Two machine-local files live beside the journal in the selected workspace's
//! UUID-scoped sync cache:
//!
//! - `current.json` — a small [`CurrentState`] record written when a sync
//!   starts and removed when it ends. Its presence (validated against the
//!   owning PID's liveness) is how `brain sync status` and a following
//!   `brain sync` know a sync is underway.
//! - `current.log` — the running sync's progress lines. The sync appends each
//!   themed phase/file line here (via [`Reporter`]); a following `brain sync`
//!   tails it to mirror the live progress without starting a second sync.
//!
//! The [`Reporter`] is the single output sink for a run: every line goes to the
//! log *and* to this process's stderr. In a foreground `brain sync` stderr is
//! the user's terminal (they watch it live); in a detached background sync
//! stderr is `/dev/null` (silent), so the log is the only record — which is
//! exactly what the follower reads. Nothing a background sync prints can ever
//! reach the TUI, because a background sync is a separate process.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The record describing the sync currently in progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentState {
    /// PID of the process running the sync (validated for liveness on read, so
    /// a hard-killed sync's stale record is never mistaken for a live one).
    pub pid: u32,
    /// `both` / `push` / `pull` / `resync`.
    pub direction: String,
    /// RFC3339 UTC start timestamp.
    pub started_at: String,
}

/// Pure staleness decision.
///
/// A sync is "in progress" only when its state record exists *and* the process
/// that owns it is still alive. A record left behind by a hard-killed sync (its
/// owner gone) reads as not-running.
#[must_use]
pub fn running(state: Option<&CurrentState>, owner_alive: bool) -> bool {
    state.is_some() && owner_alive
}

/// Read + parse the state record at `path` (`None` if absent or unparseable).
#[must_use]
pub fn read_state_at(path: &Path) -> Option<CurrentState> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Read the default state record.
#[must_use]
pub fn read_state(paths: &crate::workspace::WorkspacePaths) -> Option<CurrentState> {
    read_state_at(&paths.sync_current_state())
}

/// Whether a sync is currently in progress (default paths), validated against
/// the owning PID's liveness.
#[must_use]
pub fn is_running(paths: &crate::workspace::WorkspacePaths) -> bool {
    match read_state(paths) {
        Some(s) => {
            let alive = crate::server::lifecycle::pid_alive(s.pid);
            running(Some(&s), alive)
        }
        None => false,
    }
}

/// The single progress sink for a running sync.
///
/// Appends each line to `current.log` and echoes it to this process's stderr.
/// Writing the `current.json` record is done at [`Reporter::begin`]; [`Drop`]
/// removes it so the "in progress" flag clears even on an early return.
pub struct Reporter {
    state_path: PathBuf,
    log: Mutex<Option<File>>,
}

impl Reporter {
    /// Begin a run: (re)create the log (truncating any prior run's) and write
    /// the state record. Best-effort — a filesystem failure degrades to a
    /// reporter that still echoes to stderr but records nothing.
    #[must_use]
    pub fn begin(
        paths: &crate::workspace::WorkspacePaths,
        direction: &str,
        started_at: &str,
        pid: u32,
    ) -> Self {
        Self::begin_in(&paths.sync_dir(), direction, started_at, pid)
    }

    #[must_use]
    pub fn begin_in(base: &Path, direction: &str, started_at: &str, pid: u32) -> Self {
        let _ = fs::create_dir_all(base);
        let state_path = base.join("current.json");
        let state = CurrentState {
            pid,
            direction: direction.to_owned(),
            started_at: started_at.to_owned(),
        };
        if let Ok(json) = serde_json::to_string(&state) {
            let _ = fs::write(&state_path, json);
        }
        let log = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(base.join("current.log"))
            .ok();
        Self {
            state_path,
            log: Mutex::new(log),
        }
    }

    /// Emit one progress line: append it to the log and echo to stderr.
    pub fn line(&self, s: &str) {
        if let Ok(mut guard) = self.log.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = writeln!(file, "{s}");
                let _ = file.flush();
            }
        }
        eprintln!("{s}");
    }
}

impl Drop for Reporter {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.state_path);
    }
}

/// The live sync log a viewer should show, or `None` when no sync is running.
///
/// Only a *running* sync's log is offered: an older run's transcript answers a
/// question nobody asked ("what happened last time?") while looking like the
/// answer to the one they did ask ("what is happening now?").
#[must_use]
pub fn live_log(paths: &crate::workspace::WorkspacePaths) -> Option<String> {
    let state = read_state(paths).filter(|state| crate::server::lifecycle::pid_alive(state.pid))?;
    let body = std::fs::read_to_string(paths.sync_current_log()).unwrap_or_default();
    Some(format!("syncing now ({})\n\n{body}", state.direction))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> crate::workspace::WorkspacePaths {
        crate::workspace::WorkspacePaths::new(
            Path::new("/home/tester"),
            crate::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
                .expect("valid id"),
        )
    }

    #[test]
    fn running_requires_both_a_record_and_a_live_owner() {
        let s = CurrentState {
            pid: 123,
            direction: "both".into(),
            started_at: "2026-07-29T01:00:00Z".into(),
        };
        assert!(running(Some(&s), true));
        assert!(!running(Some(&s), false), "dead owner => not running");
        assert!(!running(None, true), "no record => not running");
    }

    #[test]
    fn selected_paths_are_under_the_workspace_sync_cache() {
        assert!(paths().sync_current_state().ends_with("sync/current.json"));
        assert!(paths().sync_current_log().ends_with("sync/current.log"));
    }

    #[test]
    fn begin_writes_a_readable_state_record_and_truncates_the_log() {
        let base = std::env::temp_dir().join(format!("brain-current-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let r = Reporter::begin_in(&base, "both", "2026-07-29T01:00:00Z", 4242);

        let got = read_state_at(&base.join("current.json")).expect("state written");
        assert_eq!(got.pid, 4242);
        assert_eq!(got.direction, "both");
        assert_eq!(got.started_at, "2026-07-29T01:00:00Z");

        r.line("phase one");
        r.line("phase two");
        let log = fs::read_to_string(base.join("current.log")).unwrap();
        assert_eq!(log, "phase one\nphase two\n");

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn dropping_the_reporter_clears_the_in_progress_record() {
        let base = std::env::temp_dir().join(format!("brain-current-drop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let state = base.join("current.json");
        {
            let _r = Reporter::begin_in(&base, "pull", "2026-07-29T01:00:00Z", 7);
            assert!(state.exists(), "record present while the sync runs");
        }
        assert!(!state.exists(), "record removed once the run ends");
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn a_fresh_run_truncates_a_previous_runs_log() {
        let base = std::env::temp_dir().join(format!("brain-current-trunc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let r1 = Reporter::begin_in(&base, "both", "t1", 1);
        r1.line("old run line");
        drop(r1);
        let r2 = Reporter::begin_in(&base, "both", "t2", 2);
        let log = fs::read_to_string(base.join("current.log")).unwrap();
        assert_eq!(log, "", "the new run starts from an empty log");
        drop(r2);
        fs::remove_dir_all(&base).ok();
    }
}
