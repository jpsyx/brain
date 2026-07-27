//! `App` brain-panel lifecycle: open/resume, close, focus, and seeding the
//! session with a prefilled prompt.
//!
//! The brain panel is a persistent, resumable `claude` session (the same
//! model as the `brain` sibling shell): opening it resumes the
//! most-recently-active free session for this shell (lock + recency), or
//! starts a fresh one; closing it ends that claude process but leaves the
//! conversation resumable next time. There is no completion view and no
//! queue — the panel simply stays open until the user closes it (Ctrl+X /
//! "Close brain") or claude exits.

use super::*;

use std::path::{Path, PathBuf};

use crate::pty_pane::PtyPane;
use crate::session::{self, AgentKind, Plan};

impl App<'_> {
    /// Whether the brain panel is on screen (a live agent PTY).
    pub(crate) fn brain_panel_open(&self) -> bool {
        self.brain.is_some()
    }

    pub(crate) fn focus_brain(&mut self) {
        if self.brain_panel_open() {
            self.alert = None;
            self.focus = Panel::Brain;
        }
    }

    /// Switch focus to the tasks panel. On a Brain → Tasks transition we
    /// also reload tasks.csv + habits.csv: brain-driven actions (defer,
    /// remove, complete) mutate the CSVs asynchronously and we have no
    /// completion signal, so the focus switch is our cue to pick up
    /// whatever changed while the user was over in the brain panel.
    pub(crate) fn focus_tasks(&mut self) {
        let was_on_brain = self.focus == Panel::Brain;
        self.alert = None;
        self.focus = Panel::Tasks;
        if was_on_brain {
            self.reload_after_brain();
        }
    }

    /// Open the brain panel (or focus it if already open). Resume the
    /// most-recently-active free session whose transcript still exists on
    /// disk and lock it; otherwise start a fresh session with a tasks-chosen
    /// id. When `prompt` is `Some`, the session is seeded with it: a fresh /
    /// resumed launch passes it as claude's initial argument, and an
    /// already-open panel has it typed into the running conversation. Opening
    /// the panel never quits the shell.
    pub(crate) fn open_or_focus_brain(&mut self, prompt: Option<&str>) {
        // Already open with a live agent: reuse the existing session — focus
        // it and, if a prompt was supplied, type it into the running
        // conversation. We never spawn a second session while one is up.
        if self.brain.as_ref().is_some_and(PtyPane::is_alive) {
            self.focus = Panel::Brain;
            self.alert = None;
            if let Some(p) = prompt {
                if let Some(pty) = self.brain.as_ref() {
                    send_prompt_to_pty(pty, p);
                }
                // The frontend-specific submit key is deferred a couple of
                // event-loop ticks (see `advance_submit_countdown`) so the
                // agent doesn't coalesce it into the pasted text.
                self.pending_brain_submit = BRAIN_SUBMIT_DELAY_TICKS;
            }
            return;
        }
        // A panel whose agent died (between the loop's auto-close tick and
        // this call) is torn down first so we don't type into a dead PTY;
        // the resume path below picks the same session back up.
        if self.brain.is_some() {
            self.close_brain();
        }

        let pid = i32::try_from(std::process::id()).unwrap_or(0);
        let mut resume = None;
        let mut skipped_missing = false;
        if self.agent_kind == AgentKind::Claude {
            for id in self.db.free_sessions_by_recency() {
                if !session_transcript_exists(&self.brain_root, &id) {
                    skipped_missing = true;
                    continue;
                }
                if self.db.claim(&id, &self.instance, pid).unwrap_or(false) {
                    resume = Some(id);
                    break;
                }
            }
        }

        let new_id = uuid::Uuid::new_v4().to_string();
        let plan = Plan::decide(resume, new_id.clone());
        self.alert = if matches!(plan, Plan::Fresh(_)) {
            if self.agent_kind == AgentKind::Claude {
                let _ = self.db.register_fresh(&new_id, &self.instance, pid);
            }
            skipped_missing.then(|| {
                "⚠ couldn't find a session to resume — starting a new brain chat".to_owned()
            })
        } else {
            None
        };

        let llm_cmd = match self.agent_kind {
            AgentKind::Claude => self.config.claude_command().to_owned(),
            AgentKind::Codex => crate::env::codex_command(),
        };
        let command =
            session::build_llm_command(&self.brain_root, self.agent_kind, &llm_cmd, &plan, prompt);
        let env = session::env_for(&self.instance, pid, &self.db_path);
        // Placeholder size; the first draw resizes the PTY to the real panel.
        self.brain =
            PtyPane::spawn_shell_command_with_env(&command, &env, &self.brain_root, 24, 80).ok();
        if self.brain.is_some() {
            self.focus = Panel::Brain;
        }
    }

    /// Close the brain panel: drop the PTY (its Drop impl kills the agent
    /// child, ending the session process), release the session lock so a
    /// later open (or another shell) can resume it via recency, hand the
    /// screen back to full-width tasks, and reload so a brain action whose
    /// effect landed right before the close shows up immediately.
    pub(crate) fn close_brain(&mut self) {
        self.brain = None;
        self.alert = None;
        self.focus = Panel::Tasks;
        let _ = self.db.release(&self.instance);
        self.reload_after_brain();
    }

    /// Re-read the CSVs after a brain interaction; route any error to
    /// the flash line so a transient load failure doesn't block the
    /// focus switch the user actually asked for.
    pub(crate) fn reload_after_brain(&mut self) {
        if let Err(e) = self.reload_tasks() {
            self.flash = Some(FlashKind::Error(format!("⚠ reload failed: {e}")));
        }
    }

    /// User-triggered refresh (the `r` hotkey). Re-reads the CSVs and
    /// flashes a confirmation so the user sees that the repaint
    /// actually happened, even when nothing visible changed.
    pub(crate) fn refresh(&mut self) {
        // A tasks session can span days. Advance `today` first if this refresh
        // crossed into a new logical day (6 AM rollover by default) so the
        // rebuilt view uses the new date, then re-open the daily-triage nudge
        // against the freshly-reloaded habits.
        let rolled = self.advance_triage_day(chrono::Local::now().naive_local());
        let reload = self.reload_tasks();
        if rolled {
            self.check_daily_triage();
        }
        self.flash = Some(match reload {
            Ok(()) => FlashKind::Info("✓ refreshed".to_string()),
            Err(e) => FlashKind::Error(format!("⚠ reload failed: {e}")),
        });
    }

    /// Advance the deferred-submit countdown one event-loop tick. When it
    /// reaches zero, send the frontend-specific submit key to the brain PTY for
    /// the prompt seeded a few ticks earlier. No-op when nothing is pending or
    /// the panel has since closed. Called once per event-loop iteration.
    pub(crate) fn tick_brain_submit(&mut self) {
        let (next, fire) = advance_submit_countdown(self.pending_brain_submit);
        self.pending_brain_submit = next;
        if fire {
            if let Some(pty) = self.brain.as_ref() {
                if pty.is_alive() {
                    pty.scroll_to_bottom();
                    pty.send(submit_key_for_agent(self.agent_kind));
                }
            }
        }
    }

    /// Send a prefilled prompt to the brain panel. Convenience wrapper that
    /// opens / focuses the panel (resuming the session) and seeds it with the
    /// prompt — used by palette actions like Defer, Start, Remove, and the
    /// agenda / triage flows.
    pub(crate) fn send_brain_prompt(&mut self, prompt: &str) {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            return;
        }
        self.open_or_focus_brain(Some(trimmed));
    }
}

/// True if `claude --resume <id>` would actually find this session: a
/// transcript `<id>.jsonl` exists on disk. We check the brain project dir
/// first (where the tasks panel's sessions live, since it always runs claude
/// in `<brain_root>`), then fall back to scanning every project dir in case
/// claude's dir-mangling differs across versions.
pub(crate) fn session_transcript_exists(brain_root: &Path, id: &str) -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let base = PathBuf::from(home).join(".claude").join("projects");
    let file = format!("{id}.jsonl");
    if base
        .join(session::project_dir_name(brain_root))
        .join(&file)
        .is_file()
    {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(&base) else {
        return false;
    };
    entries.flatten().any(|e| e.path().join(&file).is_file())
}

/// Type a prefilled prompt into an already-running agent PTY. Internal
/// newlines are sent as `Alt+Enter` (`ESC` + `CR`) — agent frontends treat
/// that as "insert newline", not "submit" — so a multi-line prompt arrives
/// intact. The submitting key is deliberately NOT appended here: frontends can
/// coalesce a burst of bytes ending in a submit key into a single paste, leaving
/// the message sitting unsent in the input. The caller defers the submit key a
/// couple of event-loop ticks (`App::pending_brain_submit` / `tick_brain_submit`)
/// so it arrives as a distinct keystroke and actually submits or queues.
pub(crate) fn send_prompt_to_pty(pty: &PtyPane, prompt: &str) {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return;
    }
    pty.scroll_to_bottom();
    let mut bytes: Vec<u8> = Vec::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch == '\n' {
            bytes.extend_from_slice(&[0x1B, b'\r']);
        } else {
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
    }
    pty.send(bytes);
}

/// Keystroke that submits or queues an injected prompt for the active frontend.
#[must_use]
pub(crate) fn submit_key_for_agent(agent_kind: AgentKind) -> Vec<u8> {
    match agent_kind {
        AgentKind::Claude => vec![b'\r'],
        AgentKind::Codex => vec![b'\t'],
    }
}

/// Event-loop ticks to wait after seeding a prompt before sending the
/// frontend-specific submit key. Each tick is ~one poll interval (~50ms), so
/// two ticks puts a comfortable gap between the pasted text and the submit key.
const BRAIN_SUBMIT_DELAY_TICKS: u8 = 2;

/// Advance the deferred-submit countdown by one tick. Returns the new count
/// and whether the submitting key should be sent now — true only on the tick
/// the count reaches zero, so the key fires exactly once.
pub(crate) const fn advance_submit_countdown(pending: u8) -> (u8, bool) {
    match pending {
        0 => (0, false),
        1 => (0, true),
        n => (n - 1, false),
    }
}
