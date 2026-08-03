//! Out-of-band sync for the two task CSVs (`tasks/tasks.csv`,
//! `tasks/habits.csv`).
//!
//! Line-based bisync of these files loses edits made on two machines between
//! syncs, so they are excluded from bisync (see [`crate::sync::args`]) and
//! reconciled here with the UUID-aware 3-way merge in [`crate::sync::csv_merge`]
//! against a machine-local cached baseline (the last-synced snapshot).
//!
//! The pure helpers ([`baseline_path`], [`remote_csv_arg`]) are unit-tested; the
//! orchestrators ([`sync_one`], [`sync_csvs`]) are thin IO shells that inject the
//! remote fetch/push so the merge logic (already tested) can be exercised over
//! local files by the integration test, and over rclone `copyto` in production.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::sync::args::Direction;
use crate::sync::config::SyncConfig;
use crate::sync::csv_merge::{
    Table, merge, parse, project_task_lists, rewrite_project_metadata, serialize,
    validate_for_merge,
};
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

    let manifest = local
        .parent()
        .map(|directory| directory.join("SCHEMA.json"))
        .and_then(|path| std::fs::read_to_string(path).ok());
    if let Err(error) = validate_for_merge(manifest.as_deref(), &[&base, &ours, &theirs]) {
        crate::logging::log(format!("csv merge refused for {name}: {error:#}"));
        return CsvMergeOutcome {
            name,
            soft_conflicts: 1,
            ..CsvMergeOutcome::default()
        };
    }

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

/// Regenerate canonical project reverse links from final CSV display IDs.
fn reconcile_project_metadata(
    root: &Path,
    tables: &[Table],
    update_local: bool,
    mut push: impl FnMut(&str, &str) -> bool,
) -> Result<usize> {
    let project_ids = project_task_lists(tables.iter());
    let mut staged = Vec::new();
    for base in [root.join("projects"), root.join("archive/projects")] {
        let Ok(entries) = std::fs::read_dir(base) else {
            continue;
        };
        let mut metadata_paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path().join(".METADATA.json"))
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        metadata_paths.sort();
        for path in metadata_paths {
            let before = std::fs::read(&path)
                .with_context(|| format!("reading project metadata {}", path.display()))?;
            let value = serde_json::from_slice::<serde_json::Value>(&before)
                .with_context(|| format!("parsing project metadata {}", path.display()))?;
            let project = value
                .get("name")
                .and_then(serde_json::Value::as_str)
                .or_else(|| path.parent()?.file_name()?.to_str())
                .unwrap_or_default();
            let ids = project_ids.get(project).map_or(&[][..], Vec::as_slice);
            let after = rewrite_project_metadata(&before, ids)
                .with_context(|| format!("rewriting project metadata {}", path.display()))?;
            if before == after {
                continue;
            }
            let relative = path.strip_prefix(root).with_context(|| {
                format!(
                    "project metadata {} is outside the workspace",
                    path.display()
                )
            })?;
            let relative = relative.to_string_lossy().replace('\\', "/");
            staged.push((path, relative, before, after));
        }
    }
    let mut written = Vec::new();
    if update_local {
        for (path, _, before, after) in &staged {
            if let Err(error) = std::fs::write(path, after) {
                for (written_path, original) in written.into_iter().rev() {
                    let _ = std::fs::write(written_path, original);
                }
                return Err(error)
                    .with_context(|| format!("writing project metadata {}", path.display()));
            }
            written.push((path, before));
        }
    }
    for (_, relative, _, after) in &staged {
        let text = String::from_utf8_lossy(after);
        if !push(relative, &text) {
            bail!("pushing reconciled project metadata {relative}");
        }
    }
    Ok(staged.len())
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
    let mut merged_tables = Vec::with_capacity(CSVS.len());
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
        let uploaded_text = std::cell::RefCell::new(None);
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
            if ok {
                uploaded_text.replace(Some(text.to_owned()));
            }
            ok
        };

        let outcome = if direction == Direction::Push {
            sync_one_push_only(paths, &local, rel, fetch, push)
        } else {
            sync_one(paths, &local, rel, fetch, push)
        };
        let merged_text = if direction == Direction::Push {
            uploaded_text
                .into_inner()
                .unwrap_or_else(|| std::fs::read_to_string(&local).unwrap_or_default())
        } else {
            std::fs::read_to_string(&local).unwrap_or_default()
        };
        merged_tables.push(parse(&merged_text));
        outcomes.push(outcome);
    }
    let metadata_push = |relative: &str, text: &str| {
        let tmp = temporary_dir.join(format!(
            "project-metadata-push-{}-{}",
            std::process::id(),
            relative.replace('/', "_")
        ));
        if std::fs::write(&tmp, text).is_err() {
            return false;
        }
        let remote_arg = remote_csv_arg(&remote.arg, relative);
        let args = [
            "copyto".to_owned(),
            tmp.to_string_lossy().into_owned(),
            remote_arg,
        ];
        let (ok, _) = run_rclone_capture(&remote.env, &args);
        let _ = std::fs::remove_file(&tmp);
        ok
    };
    if let Err(error) = reconcile_project_metadata(
        root,
        &merged_tables,
        direction != Direction::Push,
        metadata_push,
    ) {
        crate::logging::log(format!("project metadata reconciliation failed: {error:#}"));
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

    #[test]
    fn unsupported_current_schema_refuses_all_csv_writes() {
        use std::cell::Cell;

        let directory = tempfile::tempdir().unwrap();
        let tasks = directory.path().join("workspace/tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let local = tasks.join("tasks.csv");
        let text = "task_uuid,task_id,assigned_to,system_key,last_touched\n\
                    10000000-0000-4000-8000-000000000010,T10,member-a,,2026-08-02\n";
        std::fs::write(&local, text).unwrap();
        std::fs::write(
            tasks.join("SCHEMA.json"),
            r#"{"task_schema_version":3,"merge_key":"task_uuid"}"#,
        )
        .unwrap();
        let paths = paths(directory.path());
        let pushed = Cell::new(false);

        sync_one(
            &paths,
            &local,
            "tasks/tasks.csv",
            || Some(text.to_owned()),
            |_| {
                pushed.set(true);
                true
            },
        );

        assert_eq!(std::fs::read_to_string(&local).unwrap(), text);
        assert!(!pushed.get());
        assert!(!baseline_path(&paths, "tasks.csv").exists());
    }

    #[test]
    fn reconciled_project_metadata_is_written_and_pushed_with_final_ids() {
        use std::cell::RefCell;

        let directory = tempfile::tempdir().unwrap();
        let metadata = directory.path().join("projects/alpha/.METADATA.json");
        std::fs::create_dir_all(metadata.parent().unwrap()).unwrap();
        std::fs::write(
            &metadata,
            b"{\"name\":\"alpha\",\"title\":\"Alpha\",\"tasks\":[\"T10\"]}\n",
        )
        .unwrap();
        let table = parse(
            "task_uuid,task_id,project\n\
             10000000-0000-4000-8000-000000000010,T10,alpha\n\
             20000000-0000-4000-8000-000000000010,T13,alpha\n",
        );
        let pushed = RefCell::new(Vec::new());

        let changed =
            reconcile_project_metadata(directory.path(), &[table], true, |relative, text| {
                pushed
                    .borrow_mut()
                    .push((relative.to_owned(), text.to_owned()));
                true
            })
            .unwrap();

        let local: serde_json::Value =
            serde_json::from_slice(&std::fs::read(metadata).unwrap()).unwrap();
        assert_eq!(changed, 1);
        assert_eq!(local["title"], "Alpha");
        assert_eq!(local["tasks"], serde_json::json!(["T10", "T13"]));
        assert_eq!(pushed.borrow().len(), 1);
        assert_eq!(pushed.borrow()[0].0, "projects/alpha/.METADATA.json");
    }

    #[test]
    fn malformed_project_metadata_aborts_before_rewriting_unrelated_projects() {
        let directory = tempfile::tempdir().unwrap();
        let alpha = directory.path().join("projects/alpha/.METADATA.json");
        let broken = directory.path().join("projects/zeta/.METADATA.json");
        std::fs::create_dir_all(alpha.parent().unwrap()).unwrap();
        std::fs::create_dir_all(broken.parent().unwrap()).unwrap();
        let original = b"{\"name\":\"alpha\",\"tasks\":[\"T10\"]}\n";
        std::fs::write(&alpha, original).unwrap();
        std::fs::write(&broken, b"not json\n").unwrap();
        let table = parse(
            "task_uuid,task_id,project\n\
             10000000-0000-4000-8000-000000000010,T13,alpha\n",
        );

        let result = reconcile_project_metadata(directory.path(), &[table], true, |_, _| true);

        assert!(result.is_err());
        assert_eq!(std::fs::read(alpha).unwrap(), original);
    }
}
