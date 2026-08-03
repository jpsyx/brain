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

use crate::sync::args::Direction;
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
/// `<workspace-cache>/sync/baselines/<name>`. Never synced.
#[must_use]
pub fn baseline_path(paths: &crate::workspace::WorkspacePaths, name: &str) -> PathBuf {
    paths.sync_csv_baselines().join(name)
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
    paths: &crate::workspace::WorkspacePaths,
    local: &Path,
    rel: &str,
    fetch: impl Fn() -> Option<String>,
    push: impl Fn(&str) -> bool,
) -> CsvMergeOutcome {
    sync_one_with_mode(paths, local, rel, fetch, push, true)
}

fn sync_one_push_only(
    paths: &crate::workspace::WorkspacePaths,
    local: &Path,
    rel: &str,
    fetch: impl Fn() -> Option<String>,
    push: impl Fn(&str) -> bool,
) -> CsvMergeOutcome {
    sync_one_with_mode(paths, local, rel, fetch, push, false)
}

fn sync_one_with_mode(
    paths: &crate::workspace::WorkspacePaths,
    local: &Path,
    rel: &str,
    fetch: impl Fn() -> Option<String>,
    push: impl Fn(&str) -> bool,
    update_local_and_baseline: bool,
) -> CsvMergeOutcome {
    let name = Path::new(rel)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(rel)
        .to_owned();
    let baseline = baseline_path(paths, &name);

    let baseline_text = std::fs::read_to_string(&baseline).unwrap_or_default();
    let local_text = std::fs::read_to_string(local).unwrap_or_default();
    let remote_text = fetch().unwrap_or_default();
    let base = parse(&baseline_text);
    let ours = parse(&local_text);
    let theirs = parse(&remote_text);

    let (merged, report) = merge(&base, &ours, &theirs);
    let text = serialize(&merged);

    if update_local_and_baseline && local_text != text {
        write_all(local, &text);
    }
    if remote_text != text {
        push(&text);
    }
    if update_local_and_baseline && baseline_text != text {
        write_all(&baseline, &text);
    }

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
pub fn sync_csvs(
    paths: &crate::workspace::WorkspacePaths,
    cfg: &SyncConfig,
    root: &Path,
    direction: Direction,
) -> Vec<CsvMergeOutcome> {
    let remote = build_remote(cfg);
    let temporary_dir = paths.sync_dir().join("tmp");
    let _ = std::fs::create_dir_all(&temporary_dir);
    let mut outcomes = Vec::with_capacity(CSVS.len());
    for rel in CSVS {
        let local = root.join(rel);
        let remote_arg = remote_csv_arg(&remote.arg, rel);
        let tag = rel.replace('/', "_");

        let fetch = || {
            let tmp = temporary_dir.join(format!("csv-fetch-{}-{tag}", std::process::id()));
            let args = [
                "copyto".to_owned(),
                remote_arg.clone(),
                tmp.to_string_lossy().into_owned(),
            ];
            let (ok, _) = run_rclone_capture(&remote.env, &args);
            let text = ok.then(|| std::fs::read_to_string(&tmp).ok()).flatten();
            let _ = std::fs::remove_file(&tmp);
            text
        };
        let push = |text: &str| {
            let tmp = temporary_dir.join(format!("csv-push-{}-{tag}", std::process::id()));
            if std::fs::write(&tmp, text).is_err() {
                return false;
            }
            let args = [
                "copyto".to_owned(),
                tmp.to_string_lossy().into_owned(),
                remote_arg.clone(),
            ];
            let (ok, _) = run_rclone_capture(&remote.env, &args);
            let _ = std::fs::remove_file(&tmp);
            ok
        };

        outcomes.push(if direction == Direction::Push {
            sync_one_push_only(paths, &local, rel, fetch, push)
        } else {
            sync_one(paths, &local, rel, fetch, push)
        });
    }
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(home: &Path) -> crate::workspace::WorkspacePaths {
        crate::workspace::WorkspacePaths::new(home, crate::workspace::WorkspaceId::new())
    }

    #[test]
    fn baseline_path_is_under_cache_brain_sync_baselines() {
        let paths = paths(Path::new("/home/tester"));
        assert!(baseline_path(&paths, "tasks.csv").ends_with("sync/baselines/tasks.csv"));
        assert!(baseline_path(&paths, "habits.csv").ends_with("sync/baselines/habits.csv"));
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

    #[test]
    fn push_only_merge_preserves_remote_rows_without_downloading_them() {
        use std::cell::RefCell;

        let base = std::env::temp_dir().join(format!("brain-csv-push-only-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let local = base.join("local.csv");
        let rel = format!("tasks/push-only-{}.csv", std::process::id());
        let name = Path::new(&rel).file_name().unwrap().to_str().unwrap();
        let paths = paths(&base);
        let baseline = baseline_path(&paths, name);
        std::fs::remove_file(&baseline).ok();
        let header = "task_id,status,notes,last_touched\n";
        std::fs::write(&local, format!("{header}A,open,local,t1\n")).unwrap();
        let uploaded = RefCell::new(String::new());

        sync_one_push_only(
            &paths,
            &local,
            &rel,
            || Some(format!("{header}B,open,remote,t1\n")),
            |text| {
                uploaded.replace(text.to_owned());
                true
            },
        );

        let local_after = std::fs::read_to_string(&local).unwrap();
        assert!(local_after.contains("A,open,local"));
        assert!(!local_after.contains("B,open,remote"));
        let remote_after = uploaded.borrow();
        assert!(remote_after.contains("A,open,local"));
        assert!(remote_after.contains("B,open,remote"));
        assert!(
            !baseline.exists(),
            "push-only must not advance the downstream baseline"
        );

        std::fs::remove_dir_all(base).ok();
    }
}
