//! Shell-facing sync triggers.
//!
//! Every automatic trigger (startup, the filesystem watcher, and the
//! receiver freshness gate) runs the sync as a **fully detached child process**,
//! never on a thread inside the TUI. Two reasons, both required:
//!
//! 1. **The TUI must never see sync output.** A sync run on a thread inside the
//!    TUI process writes rclone's progress to that process's stderr, which
//!    bleeds over the ratatui frame on `/dev/tty`. A separate process with null
//!    stdio can't touch the TUI at all.
//! 2. **A sync must outlive the TUI.** Quitting the shell (or closing the
//!    terminal) must not kill or orphan an in-flight sync. A detached child in
//!    its own process group keeps running to completion.
//!
//! Each child runs `brain sync … --if-idle`, so if a sync is already in
//! progress it coalesces (exits silently) instead of stacking a second run. The
//! workspace-UUID lock (`lock.rs`) is the actual serializer; `--if-idle` just
//! keeps a redundant trigger from turning into a follower.

use std::process::{Command, Stdio};

use crate::sync::args::Direction;

/// Complete workspace-sensitive input for one detached sync child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachedSyncRequest {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Process-launch boundary for detached sync children.
pub trait DetachedSyncRunner: Send + Sync {
    /// Launch one fully specified child and return its process ID.
    fn spawn(&self, request: DetachedSyncRequest) -> std::io::Result<u32>;
}

struct ProcessDetachedSyncRunner;

impl DetachedSyncRunner for ProcessDetachedSyncRunner {
    fn spawn(&self, request: DetachedSyncRequest) -> std::io::Result<u32> {
        use std::os::unix::process::CommandExt as _;
        let exe = std::env::current_exe()?;
        let mut command = Command::new(exe);
        command
            .args(request.args)
            .envs(request.env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        spawn_reaped_command(command)
    }
}

#[must_use]
fn detached_sync_args(
    workspace: &crate::workspace::WorkspaceContext,
    dir: Direction,
) -> Vec<String> {
    let mut args = vec![
        "--workspace".to_owned(),
        workspace.name().as_str().to_owned(),
        "sync".to_owned(),
    ];
    match dir {
        Direction::Pull => args.push("--pull".to_owned()),
        Direction::Push => args.push("--push".to_owned()),
        Direction::Both | Direction::Resync => {}
    }
    args.push("--if-idle".to_owned());
    args
}

/// Build one detached child's canonical selector, sync arguments, and expected UUID.
#[must_use]
pub fn detached_sync_request(
    workspace: &crate::workspace::WorkspaceContext,
    dir: Direction,
) -> DetachedSyncRequest {
    DetachedSyncRequest {
        args: detached_sync_args(workspace, dir),
        env: vec![("BRAIN_WORKSPACE_ID".to_owned(), workspace.id().to_string())],
    }
}

fn spawn_reaped_command(mut command: Command) -> std::io::Result<u32> {
    let mut child = command.spawn()?;
    let pid = child.id();
    std::thread::spawn(move || {
        if let Err(error) = child.wait() {
            crate::logging::log(format!(
                "background sync child wait failed pid={pid}: {error}"
            ));
        } else {
            crate::logging::log(format!("background sync child reaped pid={pid}"));
        }
    });
    Ok(pid)
}

/// Spawn a detached, silent `brain sync` for `dir` and return immediately.
///
/// The child gets its own process group and null stdio, so it survives shell
/// teardown / terminal close and prints nothing to the terminal (its progress
/// still lands in `current.log` for `brain sync status` / a following
/// `brain sync`). Best-effort: a spawn failure is swallowed.
#[must_use]
pub fn spawn_detached_sync(
    workspace: &crate::workspace::WorkspaceContext,
    dir: Direction,
) -> Option<u32> {
    spawn_detached_sync_with(workspace, dir, &ProcessDetachedSyncRunner)
}

/// Spawn through an injected runner while preserving the production request.
#[must_use]
pub fn spawn_detached_sync_with(
    workspace: &crate::workspace::WorkspaceContext,
    dir: Direction,
    runner: &dyn DetachedSyncRunner,
) -> Option<u32> {
    crate::logging::log(format!("spawn detached sync dir={dir:?}"));
    match runner.spawn(detached_sync_request(workspace, dir)) {
        Ok(pid) => Some(pid),
        Err(error) => {
            crate::logging::log(format!("spawn detached sync failed dir={dir:?}: {error}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use serde_json::Map;

    use crate::workspace::{
        MachineRegistry, RegistryStore, WorkspaceId, WorkspaceName, WorkspaceRecord,
    };

    fn workspace() -> crate::workspace::WorkspaceContext {
        crate::workspace::WorkspaceContext::new(
            std::path::Path::new("/home/tester"),
            crate::workspace::WorkspaceId::new(),
            crate::workspace::WorkspaceName::parse("family").expect("valid name"),
            std::path::Path::new("/home/tester/family"),
            "pablo",
            std::path::Path::new("/home/tester"),
        )
        .expect("context")
    }

    #[test]
    fn detached_sync_arguments_pin_the_canonical_workspace() {
        assert_eq!(
            detached_sync_args(&workspace(), Direction::Pull),
            ["--workspace", "family", "sync", "--pull", "--if-idle"]
        );
        assert_eq!(
            detached_sync_args(&workspace(), Direction::Push),
            ["--workspace", "family", "sync", "--push", "--if-idle"]
        );
        assert_eq!(
            detached_sync_args(&workspace(), Direction::Both),
            ["--workspace", "family", "sync", "--if-idle"]
        );
    }

    #[test]
    fn alias_selected_detached_args_ignore_later_alias_removal_and_default_change() {
        let home = tempfile::tempdir().unwrap();
        let personal_root = home.path().join("personal");
        let family_root = home.path().join("family");
        std::fs::create_dir_all(&personal_root).unwrap();
        std::fs::create_dir_all(&family_root).unwrap();
        let personal_name = WorkspaceName::parse("personal").unwrap();
        let family_name = WorkspaceName::parse("family").unwrap();
        let family_id = WorkspaceId::parse("e806258e-491a-436d-9db4-a5ca9903e0d4").unwrap();
        let registry = MachineRegistry {
            schema_version: crate::workspace::REGISTRY_SCHEMA_VERSION,
            default_workspace: personal_name.clone(),
            workspaces: BTreeMap::from([
                (
                    personal_name,
                    WorkspaceRecord {
                        workspace_id: WorkspaceId::parse("8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b")
                            .unwrap(),
                        root: personal_root,
                        aliases: BTreeSet::new(),
                        local_user_id: "pablo".to_owned(),
                        receiver_enabled: false,
                        env: Map::new(),
                    },
                ),
                (
                    family_name,
                    WorkspaceRecord {
                        workspace_id: family_id,
                        root: family_root,
                        aliases: BTreeSet::from([WorkspaceName::parse("fam").unwrap()]),
                        local_user_id: "pablo".to_owned(),
                        receiver_enabled: false,
                        env: Map::new(),
                    },
                ),
            ]),
            env: serde_json::Map::new(),
        };
        let store = RegistryStore::from_path(home.path().join("config/brain/env.json"));
        store.replace(&registry).unwrap();
        let loaded = RegistryStore::load_from(store.path()).unwrap();
        let selected = loaded.select(Some("fam")).unwrap();
        let selected_context = crate::workspace::WorkspaceContext::new(
            home.path(),
            selected.record().workspace_id,
            selected.canonical_name().clone(),
            &selected.record().root,
            selected.record().local_user_id.clone(),
            home.path(),
        )
        .unwrap();

        let mut changed = RegistryStore::load_from(store.path()).unwrap();
        changed.set_default("family").unwrap();
        changed.remove_alias("family", "fam").unwrap();
        store.replace(&changed).unwrap();

        for (direction, expected) in [
            (
                Direction::Pull,
                vec!["--workspace", "family", "sync", "--pull", "--if-idle"],
            ),
            (
                Direction::Push,
                vec!["--workspace", "family", "sync", "--push", "--if-idle"],
            ),
            (
                Direction::Both,
                vec!["--workspace", "family", "sync", "--if-idle"],
            ),
        ] {
            assert_eq!(detached_sync_args(&selected_context, direction), expected);
        }
    }

    #[test]
    fn completed_background_children_are_reaped() {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 0"]);
        let pid = spawn_reaped_command(command).expect("spawn test child");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while crate::state::system_pid_alive(i32::try_from(pid).unwrap_or(0))
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(
            !crate::state::system_pid_alive(i32::try_from(pid).unwrap_or(0)),
            "a finished detached child must not remain as a zombie"
        );
    }
}
