//! Shell-facing sync triggers.
//!
//! Every automatic trigger (startup, the filesystem watcher, the idle-pull
//! timer, and shell exit) runs the sync as a **fully detached child process**,
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
//! machine-wide lock (`lock.rs`) is the actual serializer; `--if-idle` just
//! keeps a redundant trigger from turning into a follower.

use std::process::{Command, Stdio};

use crate::sync::args::Direction;

/// Spawn a detached, silent `brain sync` for `dir` and return immediately.
///
/// The child gets its own process group and null stdio, so it survives shell
/// teardown / terminal close and prints nothing to the terminal (its progress
/// still lands in `current.log` for `brain sync status` / a following
/// `brain sync`). Best-effort: a spawn failure is swallowed.
pub fn spawn_detached_sync(dir: Direction) {
    use std::os::unix::process::CommandExt as _;
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = Command::new(exe);
    cmd.arg("sync");
    match dir {
        Direction::Pull => {
            cmd.arg("--pull");
        }
        Direction::Push => {
            cmd.arg("--push");
        }
        // Both is the default direction; Resync isn't a background trigger.
        Direction::Both | Direction::Resync => {}
    }
    cmd.arg("--if-idle")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    crate::logging::log(format!("spawn detached sync dir={dir:?}"));
    let _ = cmd.spawn();
}
