//! Out-of-band sync for the two task CSVs (`tasks/tasks.csv`,
//! `tasks/habits.csv`).
//!
//! Line-based bisync of these files loses edits made on two machines between
//! syncs, so they are excluded from bisync (see [`crate::sync::args`]) and
//! reconciled here with the id-keyed 3-way merge in [`crate::sync::csv_merge`]
//! against a machine-local cached baseline (the last-synced snapshot).
//!
//! The pure helpers ([`baseline_path`], [`remote_csv_arg`]) are unit-tested; the
//! orchestrators ([`sync_one`], [`sync_csvs`]) are thin IO shells that inject the
//! remote fetch/push so the merge logic (already tested) can be exercised over
//! local files by the integration test, and over rclone `copyto` in production.

use std::path::{Path, PathBuf};

use crate::sync::config::SyncConfig;
use crate::sync::csv_merge::{merge, parse, serialize};
use crate::sync::remote::build_remote;
use crate::sync::run::run_rclone_capture;

/// The two CSVs reconciled out-of-band, as repo-relative paths.
pub const CSVS: [&str; 2] = ["tasks/tasks.csv", "tasks/habits.csv"];

/// Counts folded into the sync journal for one merged CSV.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CsvMergeOutcome {
    pub name: String,
    pub added: usize,
    pub deleted: usize,
    pub merged: usize,
    pub soft_conflicts: usize,
}

/// The machine-local baseline (last-synced snapshot) for a CSV:
/// `~/.cache/brain/sync/baselines/<name>`. Mirrors the journal's cache-dir
/// style; never synced.
#[must_use]
pub fn baseline_path(name: &str) -> PathBuf {
    let base = std::env::var_os("HOME").map_or_else(
        || PathBuf::from("."),
        |h| PathBuf::from(h).join(".cache").join("brain").join("sync").join("baselines"),
    );
    base.join(name)
}

/// Join a remote base with a CSV's repo-relative path, trimming one trailing
/// slash so `remote_csv_arg("BRAIN:bucket/pre", "tasks/tasks.csv")` is
/// `"BRAIN:bucket/pre/tasks/tasks.csv"`.
#[must_use]
pub fn remote_csv_arg(remote_arg: &str, rel: &str) -> String {
    format!("{}/{rel}", remote_arg.trim_end_matches('/'))
}

/// Sync ONE csv (`rel` = "tasks/tasks.csv") via the 3-way merge. `fetch`/`push`
/// are injected so the real path uses rclone `copyto` and tests use local files.
///
/// Reads the cached baseline (empty if none) as `base`, the `local` file (empty
/// if missing) as `ours`, and `fetch()` (empty if `None`) as `theirs`; merges,
/// writes the result back to `local`, pushes it, and refreshes the baseline.
pub fn sync_one(
    local: &Path,
    rel: &str,
    fetch: impl Fn() -> Option<String>,
    push: impl Fn(&str) -> bool,
) -> CsvMergeOutcome {
    let name = Path::new(rel).file_name().and_then(|s| s.to_str()).unwrap_or(rel).to_owned();
    let baseline = baseline_path(&name);

    let base = parse(&std::fs::read_to_string(&baseline).unwrap_or_default());
    let ours = parse(&std::fs::read_to_string(local).unwrap_or_default());
    let theirs = parse(&fetch().unwrap_or_default());

    let (merged, report) = merge(&base, &ours, &theirs);
    let text = serialize(&merged);

    write_all(local, &text);
    push(&text);
    write_all(&baseline, &text);

    CsvMergeOutcome {
        name,
        added: report.added,
        deleted: report.deleted,
        merged: report.merged,
        soft_conflicts: report.soft_conflicts.len(),
    }
}

/// Write `text` to `path`, creating parent dirs. Best-effort (errors ignored):
/// a failed write leaves the prior file/baseline in place, which the next sync
/// reconciles.
fn write_all(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}

/// Merge both task CSVs against the remote, wiring `fetch`/`push` to rclone
/// `copyto` through a temp file. Best-effort: a per-CSV failure yields empty
/// counts rather than aborting the caller's sync.
#[must_use]
pub fn sync_csvs(cfg: &SyncConfig, root: &Path) -> Vec<CsvMergeOutcome> {
    let remote = build_remote(cfg);
    let mut outcomes = Vec::with_capacity(CSVS.len());
    for rel in CSVS {
        let local = root.join(rel);
        let remote_arg = remote_csv_arg(&remote.arg, rel);
        let tag = rel.replace('/', "_");

        let fetch = || {
            let tmp = std::env::temp_dir().join(format!("brain-csv-fetch-{}-{tag}", std::process::id()));
            let args = ["copyto".to_owned(), remote_arg.clone(), tmp.to_string_lossy().into_owned()];
            let (ok, _) = run_rclone_capture(&remote.env, &args);
            let text = ok.then(|| std::fs::read_to_string(&tmp).ok()).flatten();
            let _ = std::fs::remove_file(&tmp);
            text
        };
        let push = |text: &str| {
            let tmp = std::env::temp_dir().join(format!("brain-csv-push-{}-{tag}", std::process::id()));
            if std::fs::write(&tmp, text).is_err() {
                return false;
            }
            let args = ["copyto".to_owned(), tmp.to_string_lossy().into_owned(), remote_arg.clone()];
            let (ok, _) = run_rclone_capture(&remote.env, &args);
            let _ = std::fs::remove_file(&tmp);
            ok
        };

        outcomes.push(sync_one(&local, rel, fetch, push));
    }
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_path_is_under_cache_brain_sync_baselines() {
        assert!(baseline_path("tasks.csv").ends_with(".cache/brain/sync/baselines/tasks.csv"));
        assert!(baseline_path("habits.csv").ends_with(".cache/brain/sync/baselines/habits.csv"));
    }

    #[test]
    fn remote_csv_arg_joins_and_trims_a_trailing_slash() {
        assert_eq!(
            remote_csv_arg("BRAIN:bucket/pre", "tasks/tasks.csv"),
            "BRAIN:bucket/pre/tasks/tasks.csv"
        );
        assert_eq!(
            remote_csv_arg("BRAIN:bucket/pre/", "tasks/habits.csv"),
            "BRAIN:bucket/pre/tasks/habits.csv"
        );
    }
}
