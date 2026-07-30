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
//! machine-wide lock (`lock.rs`) is the actual serializer; `--if-idle` just
//! keeps a redundant trigger from turning into a follower.

use std::process::{Command, Stdio};

use crate::sync::args::Direction;

fn spawn_reaped_command(mut command: Command) -> std::io::Result<u32> {
    let mut child = command.spawn()?;
    let pid = child.id();
    std::thread::spawn(move || {
        if let Err(error) = child.wait() {
            crate::logging::log(format!("background sync child wait failed pid={pid}: {error}"));
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
pub fn spawn_detached_sync(dir: Direction) -> Option<u32> {
    use std::os::unix::process::CommandExt as _;
    let Ok(exe) = std::env::current_exe() else {
        return None;
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
    match spawn_reaped_command(cmd) {
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
