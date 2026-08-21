use std::path::{Path, PathBuf};

use crate::sync::args::Direction;
use crate::sync::run::run_rclone_capture;

use super::{
    CSVS, CsvSyncError, CsvSyncResult, TASK_SCHEMA, fetch_remote_task_schema, remote_tasks_arg,
    sync_csvs_with_transport,
};

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
    // One staging directory, one download, one upload. Each rclone invocation
    // re-authenticates with the provider (~0.6s against B2), so a `copyto` per
    // file made the merge phase cost more in process startup than in transfer.
    let staging = temporary_dir.join(format!("csv-stage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::create_dir_all(&staging);
    let downloaded = batch_download(remote, &staging, &CSVS.map(basename_of));
    let mut pending: Vec<String> = Vec::new();
    let fetch = |relative: &str| {
        if relative == TASK_SCHEMA {
            return remote_schema.clone();
        }
        downloaded
            .then(|| std::fs::read_to_string(staging.join(basename_of(relative))).ok())
            .flatten()
    };
    let push = |relative: &str, text: &str| {
        let name = basename_of(relative);
        if std::fs::write(staging.join(name), text).is_err() {
            return false;
        }
        pending.push(name.to_owned());
        true
    };
    let result = sync_csvs_with_transport(paths, root, direction, fetch, push);
    // The merge's writes only reach the remote here, so a merge that aborted
    // publishes nothing — the same all-or-nothing boundary the per-file
    // transport had, now for one upload instead of several.
    if result.is_ok() && !pending.is_empty() && !batch_upload(remote, &staging, &pending) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(CsvSyncError::RemotePublish(pending.join(", ")));
    }
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// The name a task file has inside `tasks/`.
pub(super) fn basename_of(relative: &str) -> &str {
    Path::new(relative)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(relative)
}

/// Copy every named file out of the remote `tasks/` directory in one rclone run.
///
/// Missing names are not an error: an uninitialized remote simply has none, and
/// the merge treats an absent file as empty.
pub(super) fn batch_download(
    remote: &crate::sync::remote::Remote,
    staging: &Path,
    names: &[&str],
) -> bool {
    let Some(list) = write_files_from(staging, names) else {
        return false;
    };
    let args = [
        "copy".to_owned(),
        remote_tasks_arg(&remote.arg),
        staging.to_string_lossy().into_owned(),
        "--files-from".to_owned(),
        list.to_string_lossy().into_owned(),
    ];
    let (ok, _) = run_rclone_capture(&remote.env, &args);
    let _ = std::fs::remove_file(&list);
    ok
}

/// Publish every staged file back into the remote `tasks/` directory in one run.
fn batch_upload(remote: &crate::sync::remote::Remote, staging: &Path, names: &[String]) -> bool {
    let borrowed = names.iter().map(String::as_str).collect::<Vec<_>>();
    let Some(list) = write_files_from(staging, &borrowed) else {
        return false;
    };
    let args = [
        "copy".to_owned(),
        staging.to_string_lossy().into_owned(),
        remote_tasks_arg(&remote.arg),
        "--files-from".to_owned(),
        list.to_string_lossy().into_owned(),
    ];
    let (ok, _) = run_rclone_capture(&remote.env, &args);
    let _ = std::fs::remove_file(&list);
    ok
}

/// Write an rclone `--files-from` list beside the staging directory.
fn write_files_from(staging: &Path, names: &[&str]) -> Option<PathBuf> {
    let list = staging.with_extension("files-from");
    std::fs::write(&list, format!("{}\n", names.join("\n")))
        .ok()
        .map(|()| list)
}
