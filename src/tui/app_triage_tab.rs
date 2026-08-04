//! The ephemeral daily-triage tab: a second brain-panel session that runs a
//! `/triage` pass in the background.
//!
//! Saying "Yes" to the startup daily-triage nudge used to type `/triage` into
//! the *main* brain session, blocking it for the whole (often long, often
//! interactive) pass. Instead we spawn a dedicated, untracked session as a
//! second tab (`Alt+2`), seed it with `/triage`, and let the main session
//! (`Alt+1`) stay free. The pass is done not when the agent stops talking (it
//! may ask questions mid-pass) but when the `/triage` skill POSTs the
//! completion signal to the brain server; `tick_triage_done` polls that signal
//! and auto-closes the tab. See [`crate::triage_signal`].

use super::*;

use std::sync::Arc;

use crate::agent::{AgentController, AgentSession, HookMetadata, LaunchRequest, SessionPlan};
use crate::pty_pane::PtyPane;

#[cfg(not(test))]
fn triage_done_url(_app: &mut App<'_>) -> anyhow::Result<String> {
    crate::server::lifecycle::ensure_running()
        .map(|port| crate::server::url(port, crate::triage_signal::DONE_PATH))
}

#[cfg(test)]
fn triage_done_url(app: &mut App<'_>) -> anyhow::Result<String> {
    if let Some(url) = app.triage_done_url_override.take() {
        return Ok(url);
    }
    crate::server::lifecycle::ensure_running()
        .map(|port| crate::server::url(port, crate::triage_signal::DONE_PATH))
}

#[cfg(not(test))]
fn triage_transport(_app: &mut App<'_>) -> Box<dyn crate::agent::AgentTransport> {
    Box::new(PtyPane::new(24, 80))
}

#[cfg(test)]
fn triage_transport(app: &mut App<'_>) -> Box<dyn crate::agent::AgentTransport> {
    if let Some(transport) = app.triage_transport_override.take() {
        return transport;
    }
    Box::new(PtyPane::new(24, 80))
}

impl App<'_> {
    /// Whether the brain panel is on screen with *either* the main or the
    /// triage session (the panel occupies its half whenever one is present).
    pub(crate) fn any_brain_panel_visible(&self) -> bool {
        self.brain.is_some() || self.triage_brain.is_some()
    }

    /// The tab actually showable right now: `Triage` only while a triage
    /// session exists, else `Main`.
    pub(crate) fn effective_brain_tab(&self) -> BrainTab {
        resolve_active_tab(self.active_brain_tab, self.triage_brain.is_some())
    }

    /// The controller behind the currently-active tab, if any.
    pub(crate) fn active_brain_controller(&self) -> Option<&AgentController> {
        match self.effective_brain_tab() {
            BrainTab::Triage => self.triage_brain.as_ref(),
            BrainTab::Main => self.brain.as_ref(),
        }
    }

    /// Mutable counterpart of [`Self::active_brain_controller`] used by the
    /// per-frame terminal resize.
    pub(crate) fn active_brain_controller_mut(&mut self) -> Option<&mut AgentController> {
        match self.effective_brain_tab() {
            BrainTab::Triage => self.triage_brain.as_mut(),
            BrainTab::Main => self.brain.as_mut(),
        }
    }

    /// Select a brain-panel tab (`Alt+1` / `Alt+2`) and focus the brain panel.
    /// Selecting `Triage` is a no-op when no triage session is open; selecting
    /// any tab when the panel is closed does nothing.
    pub(crate) fn select_brain_tab(&mut self, tab: BrainTab) {
        if !self.any_brain_panel_visible() {
            return;
        }
        self.active_brain_tab = resolve_active_tab(tab, self.triage_brain.is_some());
        self.focus = Panel::Brain;
        self.alert = None;
    }

    /// Cycle the brain-panel tab (`Alt+[` previous / `Alt+]` next) and focus
    /// the panel. With only the main session open this just focuses the panel;
    /// with a triage tab open it flips between the two. Ordered `[Main, Triage]`
    /// so `next` from Main lands on triage.
    pub(crate) fn cycle_brain_tab(&mut self, forward: bool) {
        if !self.any_brain_panel_visible() {
            return;
        }
        let tabs: &[BrainTab] = if self.triage_brain.is_some() {
            &[BrainTab::Main, BrainTab::Triage]
        } else {
            &[BrainTab::Main]
        };
        let n = tabs.len();
        let current = self.effective_brain_tab();
        let idx = tabs.iter().position(|&t| t == current).unwrap_or(0);
        let next = if forward {
            (idx + 1) % n
        } else {
            (idx + n - 1) % n
        };
        self.select_brain_tab(tabs[next]);
    }

    /// Open the ephemeral daily-triage tab: ensure the internal brain server is
    /// up (so the skill's completion POST lands), spawn a fresh *untracked*
    /// session seeded with `/triage`, and focus it. Falls back to the old
    /// inline behavior (type `/triage` into the main session) if the server
    /// can't start, so triage still runs.
    pub(crate) fn open_triage_tab(&mut self) {
        // Already running a triage tab — just focus it rather than spawning a
        // second one.
        if self
            .triage_brain
            .as_ref()
            .is_some_and(AgentController::is_alive)
        {
            self.select_brain_tab(BrainTab::Triage);
            return;
        }

        let done_url = match triage_done_url(self) {
            Ok(url) => url,
            Err(error) => {
                crate::logging::log(format!(
                    "triage tab: brain server unavailable ({error}); running triage inline"
                ));
                self.send_brain_prompt("/triage");
                return;
            }
        };

        // Drop any stale completion signal so it can't immediately close the
        // tab we're about to open.
        crate::triage_signal::clear();

        let token = uuid::Uuid::new_v4().to_string();
        let session = AgentSession::new(uuid::Uuid::new_v4().to_string())
            .expect("generated triage session id");
        let capability_plan = match self.launch_capability_plan() {
            Ok(plan) => plan,
            Err(error) => {
                crate::logging::log(format!("triage tab capability resolution failed: {error}"));
                self.flash = Some(FlashKind::Error(format!(
                    "agent capabilities are invalid: {error}"
                )));
                return;
            }
        };
        let request = LaunchRequest::from_trusted_context(
            Arc::clone(&self.command_context.workspace),
            self.interactive_actor.clone(),
            SessionPlan::fresh(session),
            Some("/triage".to_owned()),
            self.config.access_mode,
        )
        .with_capability_plan(capability_plan)
        .with_hook_metadata(HookMetadata::new(vec![
            ("BRAIN_TRIAGE_DONE_URL".to_owned(), done_url),
            ("BRAIN_TRIAGE_TOKEN".to_owned(), token.clone()),
        ]));
        let transport = triage_transport(self);
        let mut controller =
            self.controller_for_transport(self.interactive_actor.clone(), transport);
        match controller.launch(&request) {
            Ok(()) => {
                self.triage_brain = Some(controller);
                self.triage_token = Some(token);
                self.active_brain_tab = BrainTab::Triage;
                self.focus = Panel::Brain;
                self.alert = None;
                crate::logging::log(format!(
                    "triage tab opened agent={}",
                    self.agent_kind.label()
                ));
            }
            Err(error) => {
                crate::logging::log(format!("triage tab start failed: {error}"));
                self.triage_brain = None;
                self.triage_token = None;
                self.active_brain_tab = BrainTab::Main;
                self.flash = Some(FlashKind::Error(format!(
                    "triage session could not start: {error}"
                )));
            }
        }
    }

    /// Close the triage tab: drop its PTY (killing the ephemeral session),
    /// clear the token + any pending signal, fall back to the main tab, and
    /// reload the CSVs (a triage pass mutates tasks/habits). Returns focus to
    /// the main session when it's open, else to the tasks panel.
    pub(crate) fn close_triage_tab(&mut self) {
        if let Some(mut controller) = self.triage_brain.take() {
            controller.shutdown();
        }
        self.triage_token = None;
        self.active_brain_tab = BrainTab::Main;
        crate::triage_signal::clear();
        self.focus = if self.brain.is_some() {
            Panel::Brain
        } else {
            Panel::Tasks
        };
        self.reload_after_brain();
    }

    /// One event-loop tick of the triage-tab auto-close. No-op unless a triage
    /// tab is open. Closes the tab when the ephemeral session exits on its own,
    /// or when the `/triage` skill's completion signal arrives carrying the
    /// matching one-time token (a stale signal with a different token is
    /// ignored).
    pub(crate) fn tick_triage_done(&mut self) {
        let Some(expected) = self.triage_token.clone() else {
            return;
        };
        if self
            .triage_brain
            .as_ref()
            .is_some_and(|controller| !controller.is_alive())
        {
            crate::logging::log("triage tab: session exited; closing");
            self.close_triage_tab();
            return;
        }
        let Some(signal) = crate::triage_signal::read_signal() else {
            return;
        };
        if signal.token != expected {
            return;
        }
        // The token matches, but the pass declared output artifacts (an
        // extension's printable, report, …) that must exist first. Hold the
        // signal — leave the file, keep the tab open — and re-check next tick,
        // so a premature POST can't close the tab before those outputs land.
        // An empty `require` list (core alone, or a fork with no extension)
        // closes immediately.
        if !crate::triage_signal::ready_to_close(&signal.require, |p| {
            std::path::Path::new(p).exists()
        }) {
            return;
        }
        crate::logging::log("triage tab: completion signal received; closing");
        self.close_triage_tab();
        self.flash = Some(FlashKind::Info("✓ daily triage complete".to_owned()));
    }
}

/// Which tab is actually showable: `Triage` only when a triage session exists,
/// else `Main`. Keeps `Alt+2` a no-op with no triage tab and stops rendering /
/// routing from ever pointing at a `Triage` tab that isn't there. Pure.
pub(crate) const fn resolve_active_tab(requested: BrainTab, has_triage: bool) -> BrainTab {
    match requested {
        BrainTab::Triage if has_triage => BrainTab::Triage,
        _ => BrainTab::Main,
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_active_tab;
    use crate::tui::BrainTab;

    #[test]
    fn triage_is_shown_only_when_a_triage_session_exists() {
        assert_eq!(resolve_active_tab(BrainTab::Triage, true), BrainTab::Triage);
        assert_eq!(resolve_active_tab(BrainTab::Triage, false), BrainTab::Main);
    }

    #[test]
    fn main_stays_main_regardless_of_triage_presence() {
        assert_eq!(resolve_active_tab(BrainTab::Main, true), BrainTab::Main);
        assert_eq!(resolve_active_tab(BrainTab::Main, false), BrainTab::Main);
    }
}
