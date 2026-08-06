//! Out-of-band sync for the two task CSVs (`tasks/tasks.csv`,
//! `tasks/habits.csv`).
//!
//! Line-based bisync of these files loses edits made on two machines between
//! syncs, so they are excluded from bisync (see [`crate::sync::args`]) and
//! reconciled here with the UUID-aware 3-way merge in [`crate::sync::csv_merge`]
//! against a machine-local cached baseline (the last-synced snapshot).

mod metadata;
mod operation;

use std::path::{Path, PathBuf};
use std::{error::Error, fmt};

use crate::sync::args::Direction;
use crate::sync::csv_merge::{merge, parse, schema_status, serialize, validate_for_merge};
use crate::sync::run::run_rclone_capture;
pub use operation::sync_csvs_with_transport;

#[cfg(test)]
use metadata::reconcile_project_metadata;

/// The two CSVs reconciled out-of-band, as repo-relative paths.
pub const CSVS: [&str; 2] = ["tasks/tasks.csv", "tasks/habits.csv"];
pub(crate) const TASK_SCHEMA: &str = "tasks/SCHEMA.json";

#[derive(Debug)]
pub(crate) struct RemoteTaskState {
    pub(crate) schema: Option<String>,
    pub(crate) has_csvs: bool,
}

/// Counts folded into the sync journal for one merged CSV.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CsvMergeOutcome {
    pub name: String,
    pub added: usize,
    pub deleted: usize,
    pub merged: usize,
    pub soft_conflicts: usize,
}

/// Next display IDs required by the reconciled task and habit tables.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DisplayIdFloors {
    pub tasks: u32,
    pub habits: u32,
}

impl DisplayIdFloors {
    #[must_use]
    pub fn for_counter(self, relative: &str) -> u32 {
        if relative.ends_with(".tasks_next_id") {
            self.tasks
        } else {
            self.habits
        }
    }
}

/// Successful whole-operation CSV reconciliation and its downstream floors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CsvSyncResult {
    pub outcomes: Vec<CsvMergeOutcome>,
    pub floors: DisplayIdFloors,
}

/// Typed whole-operation failure surfaced through `brain sync` orchestration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvSyncError {
    Preflight(String),
    LocalWrite(String),
    RemotePublish(String),
}

impl fmt::Display for CsvSyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preflight(message) => write!(formatter, "task CSV preflight failed: {message}"),
            Self::LocalWrite(message) => {
                write!(formatter, "task state local write failed: {message}")
            }
            Self::RemotePublish(relative) => {
                write!(
                    formatter,
                    "task state remote publication failed: {relative}"
                )
            }
        }
    }
}

impl Error for CsvSyncError {}

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

fn fetch_remote_task_schema_with(
    remote_arg: &str,
    temporary_dir: &Path,
    mut run: impl FnMut(&[String]) -> (bool, String),
) -> Result<RemoteTaskState, CsvSyncError> {
    let listing_args = [
        "lsf".to_owned(),
        remote_arg.to_owned(),
        "--recursive".to_owned(),
        "--files-only".to_owned(),
    ];
    let (listed, listing) = run(&listing_args);
    if !listed {
        return Err(CsvSyncError::Preflight(format!(
            "could not inspect remote task schema: {}",
            listing.trim()
        )));
    }
    let has_csvs = listing
        .lines()
        .any(|line| matches!(line.trim(), "tasks/tasks.csv" | "tasks/habits.csv"));
    if !listing.lines().any(|line| line.trim() == TASK_SCHEMA) {
        return Ok(RemoteTaskState {
            schema: None,
            has_csvs,
        });
    }

    let temporary = temporary_dir.join(format!(
        "task-schema-fetch-{}",
        crate::workspace::WorkspaceId::new()
    ));
    let fetch_args = [
        "copyto".to_owned(),
        remote_csv_arg(remote_arg, TASK_SCHEMA),
        temporary.to_string_lossy().into_owned(),
    ];
    let (fetched, output) = run(&fetch_args);
    if !fetched {
        return Err(CsvSyncError::Preflight(format!(
            "could not read listed remote task schema: {}",
            output.trim()
        )));
    }
    let result = std::fs::read_to_string(&temporary).map_err(|error| {
        CsvSyncError::Preflight(format!(
            "could not read fetched remote task schema {}: {error}",
            temporary.display()
        ))
    });
    let _ = std::fs::remove_file(temporary);
    result.map(|schema| RemoteTaskState {
        schema: Some(schema),
        has_csvs,
    })
}

pub(crate) fn inspect_remote_task_state(
    paths: &crate::workspace::WorkspacePaths,
    remote: &crate::sync::remote::Remote,
) -> Result<RemoteTaskState, CsvSyncError> {
    let temporary_dir = paths.sync_dir().join("tmp");
    std::fs::create_dir_all(&temporary_dir).map_err(|error| {
        CsvSyncError::LocalWrite(format!(
            "creating remote task schema staging directory {}: {error}",
            temporary_dir.display()
        ))
    })?;
    fetch_remote_task_schema_with(&remote.arg, &temporary_dir, |args| {
        run_rclone_capture(&remote.env, args)
    })
}

pub(crate) fn fetch_remote_task_schema(
    paths: &crate::workspace::WorkspacePaths,
    remote: &crate::sync::remote::Remote,
) -> Result<Option<String>, CsvSyncError> {
    inspect_remote_task_state(paths, remote).map(|state| state.schema)
}

/// Sync one CSV via the 3-way merge. This single-file helper remains for local
/// transport tests; production sync uses the preflighted whole operation.
pub fn sync_one(
    paths: &crate::workspace::WorkspacePaths,
    local: &Path,
    rel: &str,
    fetch: impl Fn() -> Option<String>,
    push: impl Fn(&str) -> bool,
) -> CsvMergeOutcome {
    sync_one_with_mode(paths, local, rel, fetch, push, true)
}

#[cfg(test)]
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
        .and_then(|name| name.to_str())
        .unwrap_or(rel)
        .to_owned();
    let baseline = baseline_path(paths, &name);
    let baseline_text = std::fs::read_to_string(&baseline).unwrap_or_default();
    let local_text = std::fs::read_to_string(local).unwrap_or_default();
    let remote_text = fetch().unwrap_or_default();
    let manifest = local
        .parent()
        .map(|directory| directory.join("SCHEMA.json"))
        .and_then(|path| std::fs::read_to_string(path).ok());
    let Ok(schema_status) = schema_status(manifest.as_deref()) else {
        return CsvMergeOutcome {
            name,
            soft_conflicts: 1,
            ..CsvMergeOutcome::default()
        };
    };
    let parsed = [
        ("baseline", parse(&baseline_text, schema_status)),
        ("local", parse(&local_text, schema_status)),
        ("remote", parse(&remote_text, schema_status)),
    ];
    let mut tables = parsed.into_iter().map(|(generation, result)| {
        result.map_err(|error| {
            crate::logging::log(format!(
                "csv merge refused for {name}: {generation} {rel}: {error}"
            ));
        })
    });
    let (Some(Ok(base)), Some(Ok(ours)), Some(Ok(theirs))) =
        (tables.next(), tables.next(), tables.next())
    else {
        return CsvMergeOutcome {
            name,
            soft_conflicts: 1,
            ..CsvMergeOutcome::default()
        };
    };
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

fn write_all(path: &Path, text: &str) {
    if let Some(directory) = path.parent() {
        let _ = std::fs::create_dir_all(directory);
    }
    let _ = std::fs::write(path, text);
}

/// Merge both task CSVs as one validated operation and publish their project
/// metadata through the same typed result boundary.
pub(crate) fn sync_csvs(
    paths: &crate::workspace::WorkspacePaths,
    verified: crate::sync::identity::VerifiedRemote<'_>,
    root: &Path,
    direction: Direction,
) -> Result<CsvSyncResult, CsvSyncError> {
    let remote = verified.remote();
    let temporary_dir = paths.sync_dir().join("tmp");
    let _ = std::fs::create_dir_all(&temporary_dir);
    let remote_schema = fetch_remote_task_schema(paths, remote)?;
    let fetch = |relative: &str| {
        if relative == TASK_SCHEMA {
            return remote_schema.clone();
        }
        let tag = relative.replace('/', "_");
        let tmp = temporary_dir.join(format!("csv-fetch-{}-{tag}", std::process::id()));
        let args = [
            "copyto".to_owned(),
            remote_csv_arg(&remote.arg, relative),
            tmp.to_string_lossy().into_owned(),
        ];
        let (ok, _) = run_rclone_capture(&remote.env, &args);
        let text = ok.then(|| std::fs::read_to_string(&tmp).ok()).flatten();
        let _ = std::fs::remove_file(&tmp);
        text
    };
    let push = |relative: &str, text: &str| {
        let tag = relative.replace('/', "_");
        let tmp = temporary_dir.join(format!("csv-push-{}-{tag}", std::process::id()));
        if std::fs::write(&tmp, text).is_err() {
            return false;
        }
        let args = [
            "copyto".to_owned(),
            tmp.to_string_lossy().into_owned(),
            remote_csv_arg(&remote.arg, relative),
        ];
        let (ok, _) = run_rclone_capture(&remote.env, &args);
        let _ = std::fs::remove_file(&tmp);
        ok
    };
    sync_csvs_with_transport(paths, root, direction, fetch, push)
}

#[cfg(test)]
mod tests;
