//! Gated integration test: exercises the real `rclone bisync` flow (brain's
//! own argument builder + runner + parser) between two local dirs. Runs only
//! when `rclone` is on PATH, so the default suite passes without rclone.

#[path = "sync_local/conflicts.rs"]
mod conflicts;
#[path = "sync_local/csv_merge.rs"]
mod csv_merge;
#[path = "sync_local/multi_workspace.rs"]
mod multi_workspace;
#[path = "sync_local/transport.rs"]
mod transport;

use std::path::Path;
use std::process::Command;

use brain::sync::args::{Direction, bisync_args};
use brain::sync::config::SyncConfig;
use brain::sync::current::Reporter;
use brain::sync::remote::Remote;
use brain::sync::run::run_rclone;
use brain::sync::verify::{self, Outcome};

fn rclone_available() -> bool {
    Command::new("rclone")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn cfg() -> SyncConfig {
    serde_json::from_str(r#"{"enabled":true,"b2_bucket":"unused","max_delete_percent":90}"#)
        .unwrap()
}

fn workspace_paths(
    base: &Path,
    workspace_id: brain::workspace::WorkspaceId,
) -> brain::workspace::WorkspacePaths {
    brain::workspace::WorkspacePaths::new(base, workspace_id)
}

fn run(a: &Path, b: &Path, dir: Direction) -> brain::sync::run::RunOutcome {
    let paths = workspace_paths(a.parent().unwrap(), workspace_id());
    run_for_workspace(a, b, dir, &paths, workspace_id())
}

fn run_for_workspace(
    a: &Path,
    b: &Path,
    dir: Direction,
    paths: &brain::workspace::WorkspacePaths,
    workspace_id: brain::workspace::WorkspaceId,
) -> brain::sync::run::RunOutcome {
    if dir == Direction::Resync {
        if !brain::workspace::WorkspaceManifest::path(a).exists() {
            let manifest = brain::workspace::WorkspaceManifest::new(workspace_id);
            manifest.write_new(a).unwrap();
            let remote_manifest = brain::workspace::WorkspaceManifest::path(b);
            std::fs::create_dir_all(remote_manifest.parent().unwrap()).unwrap();
            std::fs::copy(
                brain::workspace::WorkspaceManifest::path(a),
                remote_manifest,
            )
            .unwrap();
        }
        let remote = Remote {
            env: Vec::new(),
            arg: b.to_string_lossy().into_owned(),
        };
        let verified =
            brain::sync::identity::require_remote_identity(a, workspace_id, &remote).unwrap();
        brain::sync::check_access::ensure_markers(a, &verified).unwrap();
    }
    let workdir = brain::sync::run::bisync_workdir(paths);
    std::fs::create_dir_all(&workdir).ok();
    let args = bisync_args(
        &cfg(),
        &a.to_string_lossy(),
        &b.to_string_lossy(),
        &workdir.to_string_lossy(),
        dir,
    );
    let reporter = Reporter::begin(paths, "both", "t", std::process::id());
    run_rclone(&reporter, &[], &args)
}

fn workspace_id() -> brain::workspace::WorkspaceId {
    brain::workspace::WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
        .expect("valid workspace id")
}
