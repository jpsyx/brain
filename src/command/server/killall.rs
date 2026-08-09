//! Machine-wide cleanup for Brain server and TUI processes.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::server::lifecycle::pid_alive;
use crate::theme::Theme;

const GRACE_PERIOD: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessInfo {
    pid: u32,
    command: String,
}

/// Stop every currently discoverable Brain server and TUI process.
pub fn killall() -> Result<()> {
    let theme = Theme::active();
    let processes = list_processes()?;
    let lock_paths = tui_lock_paths()?;
    let mut server_pids = BTreeSet::new();
    let mut tui_pids = BTreeSet::new();

    for process in &processes {
        if is_server_process(&process.command) {
            server_pids.insert(process.pid);
        }
    }
    let process_by_pid = processes
        .iter()
        .map(|process| (process.pid, process))
        .collect::<std::collections::HashMap<_, _>>();
    for lock_path in &lock_paths {
        let Some(pid) = read_pid(lock_path) else {
            continue;
        };
        if process_by_pid
            .get(&pid)
            .is_some_and(|process| is_tui_process(&process.command))
        {
            tui_pids.insert(pid);
        }
    }

    let mut targets = server_pids.clone();
    targets.extend(tui_pids.iter().copied());
    if targets.is_empty() {
        println!("{}", theme.muted("No running Brain servers or TUIs found."));
        return Ok(());
    }

    println!("{}", theme.info("Stopping Brain servers and TUIs..."));
    for pid in &targets {
        send_signal(*pid, Signal::SIGTERM)?;
    }
    wait_for_exit(&targets);
    for pid in &targets {
        if pid_alive(*pid) {
            send_signal(*pid, Signal::SIGKILL)?;
        }
    }
    wait_for_exit(&targets);

    for path in lock_paths {
        if read_pid(&path).is_some_and(|pid| !pid_alive(pid)) {
            let _ = fs::remove_file(path);
        }
    }

    println!(
        "{}",
        theme.success(&format!(
            "Stopped {} server{} and {} TUI{}.",
            server_pids.len(),
            plural(server_pids.len()),
            tui_pids.len(),
            plural(tui_pids.len())
        ))
    );
    Ok(())
}

fn list_processes() -> Result<Vec<ProcessInfo>> {
    let output = std::process::Command::new("ps")
        .args(["-axo", "pid=,command="])
        .output()
        .context("listing local processes for brain killall")?;
    if !output.status.success() {
        bail!("listing local processes for brain killall failed");
    }
    Ok(parse_processes(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_processes(output: &str) -> Vec<ProcessInfo> {
    output
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim_start().split_once(char::is_whitespace)?;
            Some(ProcessInfo {
                pid: pid.parse().ok()?,
                command: command.trim().to_owned(),
            })
        })
        .collect()
}

fn is_server_process(command: &str) -> bool {
    is_brain_command(command)
        && command
            .split_whitespace()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == ["server", "run"])
}

fn is_tui_process(command: &str) -> bool {
    let words = command.split_whitespace().collect::<Vec<_>>();
    if !is_brain_command(command) || is_server_process(command) {
        return false;
    }
    let Some(argument) = words.get(1).copied() else {
        return true;
    };
    !matches!(
        argument,
        "version"
            | "config"
            | "env"
            | "sync"
            | "persona"
            | "personalize"
            | "skills"
            | "server"
            | "receiver"
            | "habits"
            | "check"
            | "killall"
            | "reindex"
            | "workspace"
            | "user"
    )
}

fn is_brain_command(command: &str) -> bool {
    command
        .split_whitespace()
        .next()
        .and_then(|executable| Path::new(executable).file_name())
        .is_some_and(|name| name == "brain")
}

fn tui_lock_paths() -> Result<Vec<PathBuf>> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")?;
    let workspaces = home.join(".cache").join("brain").join("workspaces");
    let Ok(entries) = fs::read_dir(workspaces) else {
        return Ok(Vec::new());
    };
    Ok(entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("tui.lock"))
        .filter(|path| path.is_file())
        .collect())
}

fn read_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn send_signal(pid: u32, signal: Signal) -> Result<()> {
    let pid = i32::try_from(pid).context("process id exceeds platform range")?;
    match kill(Pid::from_raw(pid), signal) {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
        Err(error) => Err(error).with_context(|| format!("sending {signal:?} to process {pid}")),
    }
}

fn wait_for_exit(pids: &BTreeSet<u32>) {
    let deadline = Instant::now() + GRACE_PERIOD;
    while Instant::now() < deadline && pids.iter().any(|pid| pid_alive(*pid)) {
        std::thread::sleep(POLL_INTERVAL);
    }
}

const fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::{is_server_process, is_tui_process, parse_processes};

    #[test]
    fn parses_process_table_lines_with_commands() {
        let processes = parse_processes(
            "  123 /usr/local/bin/brain server run --port 0\n  456 /usr/local/bin/brain\n",
        );
        assert_eq!(processes.len(), 2);
        assert_eq!(processes[0].pid, 123);
        assert!(is_server_process(&processes[0].command));
        assert!(is_tui_process(&processes[1].command));
    }

    #[test]
    fn ignores_other_brain_subcommands_and_commands_with_similar_names() {
        assert!(!is_server_process("/usr/local/bin/brain server status"));
        assert!(!is_tui_process("/usr/local/bin/brain killall"));
        assert!(is_tui_process("/usr/local/bin/brain --codex"));
        assert!(!is_tui_process("/usr/local/bin/brainish"));
        assert!(is_server_process("/tmp/brain server run"));
    }
}
