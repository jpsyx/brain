//! PTY-backed brain panel: spawns a shell command under a pseudoterminal,
//! parses its byte stream through `vt100`, and exposes the resulting screen
//! buffer for `tui-term` to render.
//!
//! Lifetime:
//!   - `spawn()` creates the PTY, kicks off two threads (reader → parser,
//!     writer ← channel), and a third that waits on the child and records
//!     the exit status into `exit_status`.
//!   - `send()` pushes bytes from the host's key handler into the writer
//!     channel (and thus the child's stdin).
//!   - `resize()` resizes both the kernel-side winsize and the parser grid
//!     so the child re-flows to the pane's current dimensions.
//!   - `is_alive()` polls `exit_status`; the brain shell uses this to decide
//!     whether to keep forwarding keys or show the "exited" footer.
//!
//! This is a near-verbatim port of `tasks/src/pty_pane.rs`; the two panels
//! embed Claude the same way.

use std::{
    io::{Read, Write},
    path::Path,
    sync::{Arc, RwLock, mpsc},
    thread,
};

use anyhow::{Context, Result};
use portable_pty::{
    ChildKiller, CommandBuilder, ExitStatus, MasterPty, NativePtySystem, PtySize, PtySystem,
};

/// Rows of scrollback the vt100 parser retains for the brain panel, so the
/// user can mouse-wheel back through Claude's output. The panel has no
/// native terminal scrollback (it's painted inside our alternate screen),
/// so this is the only history available; ~10k rows is plenty for a long
/// brain run and costs little memory at the panel's width.
const SCROLLBACK_LEN: usize = 10_000;

pub struct PtyPane {
    pub parser: Arc<RwLock<vt100::Parser>>,
    writer_tx: mpsc::Sender<Vec<u8>>,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    exit_status: Arc<RwLock<Option<ExitStatus>>>,
    pub rows: u16,
    pub cols: u16,
}

impl PtyPane {
    /// Spawn `shell -ic <command>` so that aliases / shell functions defined
    /// in the user's interactive rc resolve the same way they do at the
    /// prompt. Extra `env` vars are injected into the child; brain uses them
    /// to propagate `BRAIN_INSTANCE_ID` / `BRAIN_PID` / `BRAIN_STATE_DB` down
    /// into claude so the SessionStart hook can attribute the session.
    pub fn spawn_shell_command_with_env(
        command: &str,
        env: &[(String, String)],
        cwd: &Path,
        rows: u16,
        cols: u16,
    ) -> Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.args(["-ic", command]);
        // Start the child in the brain root so claude resolves its project
        // dir (and the `.claude/settings.json` hook) there from the first
        // instant, before the command's own `cd` even runs.
        cmd.cwd(cwd);
        // claude (and most TUIs spawned underneath) look at TERM to pick
        // capabilities; xterm-256color is a safe lowest common denominator
        // that vt100 emulates well.
        cmd.env("TERM", "xterm-256color");
        for (k, v) in env {
            cmd.env(k, v);
        }
        Self::spawn(cmd, rows, cols)
    }

    fn spawn(cmd: CommandBuilder, rows: u16, cols: u16) -> Result<Self> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow::anyhow!("openpty failed: {e}"))?;

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| anyhow::anyhow!("spawn_command failed: {e}"))?;
        // Keep `slave` alive only long enough to spawn; dropping it lets the
        // child see EOF on its controlling tty when it exits.
        drop(pair.slave);

        let killer = child.clone_killer();
        let parser = Arc::new(RwLock::new(vt100::Parser::new(rows, cols, SCROLLBACK_LEN)));
        let exit_status: Arc<RwLock<Option<ExitStatus>>> = Arc::new(RwLock::new(None));

        // Reader: PTY master → vt100 parser.
        let mut reader = pair
            .master
            .try_clone_reader()
            .context("try_clone_reader failed")?;
        {
            let parser = Arc::clone(&parser);
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut p) = parser.write() {
                                p.process(&buf[..n]);
                            }
                        }
                    }
                }
            });
        }

        // Writer: mpsc channel → PTY master.
        let (writer_tx, writer_rx) = mpsc::channel::<Vec<u8>>();
        {
            let mut writer = pair.master.take_writer().context("take_writer failed")?;
            thread::spawn(move || {
                while let Ok(bytes) = writer_rx.recv() {
                    if writer.write_all(&bytes).is_err() {
                        break;
                    }
                    let _ = writer.flush();
                }
            });
        }

        // Waiter: child.wait() → exit_status.
        {
            let exit_status = Arc::clone(&exit_status);
            thread::spawn(move || {
                if let Ok(status) = child.wait() {
                    if let Ok(mut slot) = exit_status.write() {
                        *slot = Some(status);
                    }
                }
            });
        }

        Ok(Self {
            parser,
            writer_tx,
            master: pair.master,
            killer,
            exit_status,
            rows,
            cols,
        })
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let Ok(mut p) = self.parser.write() {
            p.set_size(rows, cols);
        }
    }

    pub fn send(&self, bytes: Vec<u8>) {
        let _ = self.writer_tx.send(bytes);
    }

    /// Current scrollback offset: rows above the live tail currently in
    /// view. `0` means pinned to the bottom (live output). Test-only — the
    /// event loop scrolls blindly via `scroll_up` / `scroll_down`.
    #[cfg(test)]
    #[must_use]
    pub fn scrollback_offset(&self) -> usize {
        self.parser.read().map_or(0, |p| p.screen().scrollback())
    }

    /// Scroll the view `n` rows up into history. vt100 clamps the request
    /// to the actual amount of scrollback available, so over-scrolling
    /// stops cleanly at the top.
    pub fn scroll_up(&self, n: usize) {
        if let Ok(mut p) = self.parser.write() {
            let target = p.screen().scrollback().saturating_add(n);
            p.set_scrollback(target);
        }
    }

    /// Scroll the view `n` rows back down toward the live tail, saturating
    /// at `0` (the live output).
    pub fn scroll_down(&self, n: usize) {
        if let Ok(mut p) = self.parser.write() {
            let target = p.screen().scrollback().saturating_sub(n);
            p.set_scrollback(target);
        }
    }

    /// Snap back to the live tail. Called when a key is forwarded to the
    /// brain panel so typing always jumps the user to the prompt.
    pub fn scroll_to_bottom(&self) {
        if let Ok(mut p) = self.parser.write() {
            p.set_scrollback(0);
        }
    }

    /// Snapshot the visible terminal contents for a completed remote reply.
    #[must_use]
    pub fn contents(&self) -> String {
        self.parser
            .read()
            .map_or_else(|_| String::new(), |parser| parser.screen().contents())
    }

    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.exit_status.read().is_ok_and(|s| s.is_none())
    }
}

impl Drop for PtyPane {
    fn drop(&mut self) {
        // Best-effort: signal the child if it's still alive so the reader /
        // writer threads can wind down.
        let _ = self.killer.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Run a command in a small PTY and block until the child exits and its
    /// output has been parsed into the vt100 screen + scrollback.
    fn run_and_settle(command: &str, rows: u16, cols: u16) -> PtyPane {
        let pty = PtyPane::spawn_shell_command_with_env(command, &[], Path::new("."), rows, cols)
            .expect("spawn pty");
        for _ in 0..300 {
            if !pty.is_alive() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        // Let the reader thread drain the final bytes after EOF.
        thread::sleep(Duration::from_millis(80));
        pty
    }

    #[test]
    fn scroll_up_enters_scrollback_and_scroll_down_returns() {
        // 200 lines into a 5-row terminal pushes ~195 rows into scrollback.
        let pty = run_and_settle("seq 1 200", 5, 20);
        assert_eq!(pty.scrollback_offset(), 0, "starts pinned to the live tail");

        pty.scroll_up(10);
        assert_eq!(pty.scrollback_offset(), 10);

        pty.scroll_down(4);
        assert_eq!(pty.scrollback_offset(), 6);

        // Over-scrolling down saturates at the live tail (0).
        pty.scroll_down(1000);
        assert_eq!(pty.scrollback_offset(), 0);
    }

    #[test]
    fn scroll_up_is_clamped_to_available_scrollback() {
        let pty = run_and_settle("seq 1 200", 5, 20);
        // Asking for far more than exists clamps to the real scrollback
        // length rather than running off the end.
        pty.scroll_up(1_000_000);
        let max = pty.scrollback_offset();
        assert!(max > 0, "should have real scrollback to enter");
        // Asking again past the top is idempotent (already clamped).
        pty.scroll_up(1_000_000);
        assert_eq!(pty.scrollback_offset(), max);
    }
}
