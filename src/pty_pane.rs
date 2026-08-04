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
//! The transport is frontend-neutral: adapters supply a complete launch spec,
//! and this module owns only the pseudoterminal process and byte stream.

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

use crate::agent::{AgentError, AgentTransport, InputSequence, LaunchSpec};

/// Rows of scrollback the vt100 parser retains for the brain panel, so the
/// user can mouse-wheel back through the agent's output. The panel has no
/// native terminal scrollback (it's painted inside our alternate screen),
/// so this is the only history available; ~10k rows is plenty for a long
/// brain run and costs little memory at the panel's width.
const SCROLLBACK_LEN: usize = 10_000;

pub struct PtyPane {
    pub parser: Arc<RwLock<vt100::Parser>>,
    writer_tx: Option<mpsc::Sender<Vec<u8>>>,
    master: Option<Box<dyn MasterPty + Send>>,
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    exit_status: Option<Arc<RwLock<Option<ExitStatus>>>>,
    pub rows: u16,
    pub cols: u16,
}

impl PtyPane {
    /// Construct a dormant PTY transport with its initial terminal size.
    #[must_use]
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: Arc::new(RwLock::new(vt100::Parser::new(rows, cols, SCROLLBACK_LEN))),
            writer_tx: None,
            master: None,
            killer: None,
            exit_status: None,
            rows,
            cols,
        }
    }

    /// Spawn `shell -ic <command>` so that aliases / shell functions defined
    /// in the user's interactive rc resolve the same way they do at the
    /// prompt. Extra `env` vars are injected into the child; brain uses them
    /// to propagate the selected workspace/actor identity plus
    /// `BRAIN_INSTANCE_ID` / `BRAIN_PID` / `BRAIN_STATE_DB` into the agent so the
    /// SessionStart hook can attribute the session to the selected state DB.
    pub fn spawn_shell_command_with_env(
        command: &str,
        env: &[(String, String)],
        cwd: &Path,
        rows: u16,
        cols: u16,
    ) -> Result<Self> {
        let mut pane = Self::new(rows, cols);
        pane.start_shell_command(command, env, cwd)?;
        Ok(pane)
    }

    fn start_shell_command(
        &mut self,
        command: &str,
        env: &[(String, String)],
        cwd: &Path,
    ) -> Result<()> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.args(["-ic", command]);
        // Start the child in the selected workspace from the first instant,
        // before a compatibility command's own `cd` can run.
        cmd.cwd(cwd);
        // Agent frontends (and most TUIs spawned underneath) look at TERM to pick
        // capabilities; xterm-256color is a safe lowest common denominator
        // that vt100 emulates well.
        cmd.env("TERM", "xterm-256color");
        for (k, v) in env {
            cmd.env(k, v);
        }
        self.start(cmd)
    }

    fn start(&mut self, cmd: CommandBuilder) -> Result<()> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows: self.rows,
                cols: self.cols,
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
        let parser = Arc::new(RwLock::new(vt100::Parser::new(
            self.rows,
            self.cols,
            SCROLLBACK_LEN,
        )));
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

        self.parser = parser;
        self.writer_tx = Some(writer_tx);
        self.master = Some(pair.master);
        self.killer = Some(killer);
        self.exit_status = Some(exit_status);
        Ok(())
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        if let Some(master) = self.master.as_ref() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        if let Ok(mut p) = self.parser.write() {
            p.set_size(rows, cols);
        }
    }

    pub fn send(&self, bytes: Vec<u8>) {
        if let Some(writer_tx) = self.writer_tx.as_ref() {
            let _ = writer_tx.send(bytes);
        }
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
        self.exit_status
            .as_ref()
            .is_some_and(|status| status.read().is_ok_and(|slot| slot.is_none()))
    }
}

impl AgentTransport for PtyPane {
    fn spawn(&mut self, spec: &LaunchSpec) -> Result<(), AgentError> {
        if self.is_alive() {
            return Err(AgentError::Transport(
                "cannot replace a running PTY child".to_owned(),
            ));
        }
        self.start_shell_command(&spec.command, &spec.environment, &spec.cwd)
            .map_err(|error| AgentError::Transport(format!("{error:#}")))
    }

    fn send(&mut self, input: InputSequence) -> Result<(), AgentError> {
        if !self.is_alive() {
            return Err(AgentError::Transport("PTY child is not running".to_owned()));
        }
        let writer_tx = self
            .writer_tx
            .as_ref()
            .ok_or_else(|| AgentError::Transport("PTY child is not running".to_owned()))?;
        writer_tx
            .send(input.into_bytes())
            .map_err(|_| AgentError::Transport("PTY input channel is closed".to_owned()))
    }

    fn snapshot(&self) -> String {
        self.contents()
    }

    fn is_alive(&self) -> bool {
        Self::is_alive(self)
    }

    fn shutdown(&mut self) {
        if let Some(killer) = self.killer.as_mut() {
            let _ = killer.kill();
        }
    }

    fn terminal_screen(&self) -> Option<Arc<RwLock<vt100::Parser>>> {
        Some(Arc::clone(&self.parser))
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        Self::resize(self, rows, cols);
    }

    fn scroll_up(&mut self, rows: usize) {
        Self::scroll_up(self, rows);
    }

    fn scroll_down(&mut self, rows: usize) {
        Self::scroll_down(self, rows);
    }

    fn scroll_to_bottom(&mut self) {
        Self::scroll_to_bottom(self);
    }

    fn terminal_rows(&self) -> u16 {
        self.rows
    }
}

impl Drop for PtyPane {
    fn drop(&mut self) {
        // Best-effort: signal the child if it's still alive so the reader /
        // writer threads can wind down.
        if let Some(killer) = self.killer.as_mut() {
            let _ = killer.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentTransport, HookMetadata, LaunchSpec};
    use std::time::Duration;

    fn spec(command: &str, cwd: &Path) -> LaunchSpec {
        LaunchSpec::new(command, cwd.to_path_buf(), Vec::new(), HookMetadata::none())
    }

    fn wait_until_stopped(pty: &PtyPane) {
        for _ in 0..300 {
            if !AgentTransport::is_alive(pty) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("PTY child did not stop");
    }

    fn wait_for_file(path: &Path) {
        for _ in 0..300 {
            if path.is_file() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("PTY child did not create {}", path.display());
    }

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
    fn transport_spawns_from_the_complete_launch_spec() {
        let directory = tempfile::tempdir().expect("temporary cwd");
        let spec = LaunchSpec::new(
            "printf '%s\\n' \"$PWD\"; printf '%s' \"$BRAIN_TRANSPORT_MARKER\"",
            directory.path().to_path_buf(),
            vec![(
                "BRAIN_TRANSPORT_MARKER".to_owned(),
                "launch-spec-env".to_owned(),
            )],
            HookMetadata::none(),
        );
        let mut pty = PtyPane::new(5, 80);

        AgentTransport::spawn(&mut pty, &spec).expect("spawn through transport");
        for _ in 0..300 {
            if !AgentTransport::is_alive(&pty) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        thread::sleep(Duration::from_millis(80));

        let output = AgentTransport::snapshot(&pty);
        assert!(output.contains(&directory.path().display().to_string()));
        assert!(output.contains("launch-spec-env"));
    }

    #[test]
    fn dormant_transport_has_inert_lifecycle_and_rejects_input() {
        let mut pty = PtyPane::new(5, 80);

        assert!(!AgentTransport::is_alive(&pty));
        assert_eq!(AgentTransport::snapshot(&pty), "");
        assert_eq!(
            AgentTransport::send(&mut pty, InputSequence::bytes(b"ignored")),
            Err(AgentError::Transport("PTY child is not running".to_owned()))
        );
        AgentTransport::shutdown(&mut pty);
        AgentTransport::shutdown(&mut pty);
        assert!(!AgentTransport::is_alive(&pty));
    }

    #[test]
    fn transport_rejects_input_after_the_child_exits() {
        let mut pty = PtyPane::new(5, 80);
        AgentTransport::spawn(&mut pty, &spec("true", Path::new("."))).expect("spawn child");
        wait_until_stopped(&pty);

        assert_eq!(
            AgentTransport::send(&mut pty, InputSequence::bytes(b"too late")),
            Err(AgentError::Transport("PTY child is not running".to_owned()))
        );
    }

    #[test]
    fn transport_rejects_a_second_spawn_while_the_child_is_alive() {
        let mut pty = PtyPane::new(5, 80);
        let running = spec("sleep 30", Path::new("."));
        AgentTransport::spawn(&mut pty, &running).expect("spawn child");

        assert_eq!(
            AgentTransport::spawn(&mut pty, &spec("true", Path::new("."))),
            Err(AgentError::Transport(
                "cannot replace a running PTY child".to_owned()
            ))
        );
        AgentTransport::shutdown(&mut pty);
        wait_until_stopped(&pty);
    }

    #[test]
    fn spawn_failure_leaves_the_transport_dormant_and_reusable() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let mut pty = PtyPane::new(5, 80);
        let missing_shell = temporary.path().join("missing-shell");

        assert!(pty.start(CommandBuilder::new(missing_shell)).is_err());
        assert!(!AgentTransport::is_alive(&pty));
        assert_eq!(
            AgentTransport::send(&mut pty, InputSequence::bytes(b"ignored")),
            Err(AgentError::Transport("PTY child is not running".to_owned()))
        );

        AgentTransport::spawn(&mut pty, &spec("true", temporary.path()))
            .expect("spawn after failure");
        wait_until_stopped(&pty);
    }

    #[test]
    fn shutdown_stops_the_child_and_rejects_later_input() {
        let mut pty = PtyPane::new(5, 80);
        AgentTransport::spawn(&mut pty, &spec("sleep 30", Path::new("."))).expect("spawn child");

        AgentTransport::shutdown(&mut pty);
        wait_until_stopped(&pty);
        assert_eq!(
            AgentTransport::send(&mut pty, InputSequence::bytes(b"too late")),
            Err(AgentError::Transport("PTY child is not running".to_owned()))
        );
    }

    #[test]
    fn dropping_the_transport_stops_its_child() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let heartbeat = temporary.path().join("heartbeat");
        let command = format!(
            "while :; do printf x >> {}; sleep 0.02; done",
            crate::agent::frontend::shell_quote(&heartbeat.display().to_string())
        );
        let mut pty = PtyPane::new(5, 80);
        AgentTransport::spawn(&mut pty, &spec(&command, temporary.path())).expect("spawn child");
        wait_for_file(&heartbeat);

        drop(pty);
        thread::sleep(Duration::from_millis(100));
        let settled_size = std::fs::metadata(&heartbeat)
            .expect("heartbeat metadata")
            .len();
        thread::sleep(Duration::from_millis(200));
        let final_size = std::fs::metadata(&heartbeat)
            .expect("heartbeat metadata")
            .len();
        assert_eq!(
            final_size, settled_size,
            "PTY child remained active after drop"
        );
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
