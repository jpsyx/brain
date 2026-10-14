use std::path::{Path, PathBuf};

use crate::sync::args::Direction;
use crate::sync::run::run_rclone_capture;

use super::{
    CSVS, CsvSyncError, CsvSyncResult, TASK_SCHEMA, fetch_remote_task_schema,
    operation::sync_csvs_with_transport_owned, remote_tasks_arg,
};

/// Merge both task CSVs as one validated operation and publish their project
/// metadata through the same typed result boundary.
pub(crate) fn sync_csvs(
    workspace: &crate::workspace::WorkspaceContext,
    owner: &crate::tasks::store_lock::TaskStoreOwner,
    verified: crate::sync::identity::VerifiedRemote<'_>,
    direction: Direction,
) -> Result<CsvSyncResult, CsvSyncError> {
    owner
        .recover(workspace)
        .map_err(|error| CsvSyncError::Preflight(format!("task-store recovery: {error:#}")))?;
    let paths = workspace.paths();
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
        let destination = staging.join(relative);
        let Some(parent) = destination.parent() else {
            return false;
        };
        if std::fs::create_dir_all(parent).is_err() || std::fs::write(destination, text).is_err() {
            return false;
        }
        pending.push(relative.to_owned());
        true
    };
    let result = sync_csvs_with_transport_owned(workspace, owner, direction, fetch, push);
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

/// Publish staged workspace-relative paths without flattening project metadata.
fn batch_upload(remote: &crate::sync::remote::Remote, staging: &Path, names: &[String]) -> bool {
    let borrowed = names.iter().map(String::as_str).collect::<Vec<_>>();
    let Some(list) = write_files_from(staging, &borrowed) else {
        return false;
    };
    let args = [
        "copy".to_owned(),
        staging.to_string_lossy().into_owned(),
        remote.arg.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_transport_publishes_distinct_metadata_at_workspace_paths() {
        if !crate::sync::run::rclone_present() {
            eprintln!("skipping: rclone is not installed");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        let remote_root = temp.path().join("remote");
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(remote_root.join("tasks")).unwrap();
        for name in ["tasks.csv", "habits.csv"] {
            std::fs::write(root.join("tasks").join(name), "task_id,task_name,project\n").unwrap();
        }
        for (relative, title) in [
            ("projects/alpha/.METADATA.json", "Alpha"),
            ("archive/projects/beta/.METADATA.json", "Beta"),
        ] {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                path,
                format!(r#"{{"name":"sample","title":"{title}","tasks":[]}}"#),
            )
            .unwrap();
        }
        let workspace = crate::workspace::WorkspaceContext::new(
            temp.path(),
            crate::workspace::WorkspaceId::new(),
            crate::workspace::WorkspaceName::parse("sample").unwrap(),
            &root,
            "member",
            temp.path(),
        )
        .unwrap();
        let remote = crate::sync::remote::Remote {
            env: vec![],
            arg: remote_root.to_string_lossy().into_owned(),
        };
        let owner = crate::tasks::store_lock::TaskStoreOwner::acquire(&workspace).unwrap();
        sync_csvs(
            &workspace,
            &owner,
            crate::sync::identity::verified_remote_for_test(&remote),
            Direction::Both,
        )
        .unwrap();
        for (relative, title) in [
            ("projects/alpha/.METADATA.json", "Alpha"),
            ("archive/projects/beta/.METADATA.json", "Beta"),
        ] {
            let path = remote_root.join(relative);
            assert!(
                path.exists(),
                "publication lost workspace-relative path {relative}"
            );
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(value["title"], title);
        }
        assert!(remote_root.join("tasks/tasks.csv").exists());
        assert!(remote_root.join("tasks/habits.csv").exists());
        assert!(!remote_root.join("tasks/.METADATA.json").exists());
    }
}
