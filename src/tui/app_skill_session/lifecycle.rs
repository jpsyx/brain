//! Starting, closing, and auto-closing a skill-session tab.
//!
//! The tab *strip* (which tab is showable, the `Alt+<digit>` slots, the cycle)
//! lives in the parent module; this file owns the session's life: launching an
//! ephemeral untracked controller seeded with the session's prompt plus the
//! completion protocol, tearing one down, and the per-tick poll that closes a tab
//! when its run signals completion or its child exits.

use super::*;

use std::sync::Arc;

use crate::agent::{AgentSession, HookMetadata, LaunchRequest, SessionPlan};
use crate::pty_pane::PtyPane;

#[cfg(not(test))]
fn session_done_url(app: &App) -> anyhow::Result<String> {
    let record = crate::server::lifecycle::ServerClient::default().connect_existing()?;
    Ok(app.context.session_done_url(record.port))
}

#[cfg(test)]
fn session_done_url(app: &mut App) -> anyhow::Result<String> {
    if let Some(url) = app.brain.take_session_done_url() {
        return Ok(url);
    }
    let record = crate::server::lifecycle::ServerClient::default().connect_existing()?;
    Ok(app.context.session_done_url(record.port))
}

#[cfg(not(test))]
fn session_transport(_app: &mut App) -> Box<dyn crate::agent::AgentTransport> {
    Box::new(PtyPane::new(24, 80))
}

#[cfg(test)]
fn session_transport(app: &mut App) -> Box<dyn crate::agent::AgentTransport> {
    if let Some(transport) = app.brain.take_session_transport() {
        return transport;
    }
    Box::new(PtyPane::new(24, 80))
}

impl App {
    /// Start the builtin daily-triage skill session. The Yes-path of the startup
    /// nudge; equivalent to its command-palette row.
    pub(crate) fn open_triage_tab(&mut self) {
        self.open_skill_session(&SkillSessionSpec::daily_triage());
    }

    /// Start a skill session by definition (the palette's "Run …" row).
    pub(crate) fn run_skill_session(&mut self, key: SkillSessionKey) {
        let Some(spec) = self
            .available_skill_sessions()
            .into_iter()
            .find(|spec| spec.key == key)
        else {
            crate::logging::log(format!("skill session {key:?} is no longer configured"));
            return;
        };
        self.open_skill_session(&spec);
    }

    /// Open a skill session's tab: attach to the TUI-owned shared process, spawn
    /// a fresh *untracked* session seeded with the session's prompt plus the
    /// completion protocol, and focus it. An already-running session is focused
    /// rather than started twice. Falls back to sending the prompt to the main
    /// session if the live completion route is unavailable, so the run still
    /// happens.
    pub(crate) fn open_skill_session(&mut self, spec: &SkillSessionSpec) {
        if let Some(id) = self.brain.skill_session_id(spec.key) {
            self.select_brain_tab(BrainTab::Session(id));
            return;
        }

        let done_url = match session_done_url(self) {
            Ok(url) => url,
            Err(error) => {
                crate::logging::log(format!(
                    "skill session: brain server unavailable ({error}); running inline"
                ));
                self.send_brain_prompt(&spec.prompt);
                return;
            }
        };

        let token = uuid::Uuid::new_v4().to_string();
        // Drop any stale signal for this token's slot before the tab exists, so
        // nothing left behind can close the tab we're about to open.
        crate::skill_session::signal::clear(self.context.workspace(), &token);

        let session =
            AgentSession::new(uuid::Uuid::new_v4().to_string()).expect("generated session id");
        let capability_plan = match self.launch_capability_plan() {
            Ok(plan) => plan,
            Err(error) => {
                crate::logging::log(format!(
                    "skill session capability resolution failed: {error}"
                ));
                self.status.set_flash(FlashKind::Error(format!(
                    "agent capabilities are invalid: {error}"
                )));
                return;
            }
        };
        let mut request = LaunchRequest::from_trusted_context(
            Arc::clone(&self.context.command().workspace),
            self.brain.interactive_actor().clone(),
            SessionPlan::fresh(session),
            Some(crate::skill_session::prompt::launch_prompt(&spec.prompt)),
            self.context.access_mode(),
        );
        if let Some(plan) = capability_plan {
            request = request.with_capability_plan(plan);
        }
        request = request.with_hook_metadata(HookMetadata::new(vec![
            (
                crate::skill_session::prompt::DONE_URL_ENV.to_owned(),
                done_url,
            ),
            (
                crate::skill_session::prompt::TOKEN_ENV.to_owned(),
                token.clone(),
            ),
        ]));
        let transport = session_transport(self);
        let mut controller =
            self.controller_for_transport(self.brain.interactive_actor().clone(), transport);
        match controller.launch(&request) {
            Ok(()) => {
                let id = match self.brain.add_skill_session(
                    spec.key,
                    spec.title.clone(),
                    token.clone(),
                    controller,
                ) {
                    Ok(id) => id,
                    Err(error) => {
                        crate::logging::log(format!(
                            "skill session tab allocation failed: {error}"
                        ));
                        crate::skill_session::signal::clear(self.context.workspace(), &token);
                        self.status.set_flash(FlashKind::Error(format!(
                            "{} could not open: {error}",
                            spec.title
                        )));
                        return;
                    }
                };
                let open = self.brain.skill_session_tab_ids();
                self.shell
                    .select_brain_tab(BrainTab::Session(id), &open, true);
                self.status.clear_alert();
                crate::logging::log(format!(
                    "skill session opened title={} agent={}",
                    spec.title,
                    self.context.agent_kind().label()
                ));
            }
            Err(error) => {
                // The tab was never added, so whatever is showing stays showing;
                // only the flash reports the failure.
                crate::logging::log(format!("skill session start failed: {error}"));
                crate::skill_session::signal::clear(self.context.workspace(), &token);
                self.status.set_flash(FlashKind::Error(format!(
                    "{} could not start: {error}",
                    spec.title
                )));
            }
        }
    }

    /// Close one skill-session tab: drop its PTY (killing the ephemeral
    /// session), clear its pending signal, and reload the CSVs (a run may have
    /// mutated tasks/habits).
    ///
    /// Only the tab the user is *looking at* changes what is showing: closing the
    /// active tab falls back to the main session (or the tasks panel when no main
    /// session is open), while a background session finishing leaves the current
    /// tab and focus exactly where they were.
    pub(crate) fn close_skill_session(&mut self, id: SessionTabId) {
        let was_showing = self.effective_brain_tab() == BrainTab::Session(id);
        let Some(removed) = self.brain.remove_skill_session(id) else {
            return;
        };
        crate::skill_session::signal::clear(self.context.workspace(), &removed.token);
        if was_showing {
            let open = self.brain.skill_session_tab_ids();
            self.shell.select_brain_tab(
                BrainTab::Main,
                &open,
                self.brain.main_controller().is_some(),
            );
            if self.brain.main_controller().is_none() {
                self.shell.focus_tasks();
            }
        }
        self.reload_after_brain();
    }

    /// Close the skill-session tab currently showing, if any (`Ctrl+X` / `Esc`
    /// on a session tab). Leaves the main session untouched.
    pub(crate) fn close_active_skill_session(&mut self) {
        if let BrainTab::Session(id) = self.effective_brain_tab() {
            self.close_skill_session(id);
        }
    }

    /// Attach an already-built controller as a skill-session tab (tests only).
    #[cfg(test)]
    pub(crate) fn insert_test_skill_session(
        &mut self,
        key: SkillSessionKey,
        title: &str,
        token: &str,
        controller: AgentController,
    ) -> SessionTabId {
        let id = self
            .brain
            .add_skill_session(key, title.to_owned(), token.to_owned(), controller)
            .expect("test skill-session tab identity");
        let open = self.brain.skill_session_tab_ids();
        self.shell
            .select_brain_tab(BrainTab::Session(id), &open, true);
        id
    }

    /// The workspace's configured skill sessions, as if read from env (tests
    /// only — the real value is read once at startup).
    #[cfg(test)]
    pub(crate) fn set_test_configured_skill_sessions(&mut self, configured: serde_json::Value) {
        self.brain.set_configured_skill_sessions(configured);
    }

    /// One event-loop tick of the skill-session auto-close. No-op with no tabs
    /// open. Closes a tab when its ephemeral session exits on its own, or when
    /// its run's completion signal arrives carrying that tab's one-time token (a
    /// signal for another tab, or a stale one from an earlier run, is ignored).
    pub(crate) fn tick_skill_sessions(&mut self) {
        let mut exited = Vec::new();
        let mut completed = Vec::new();
        for session in self.brain.skill_session_observations() {
            if session.exited {
                exited.push((session.id, session.title));
                continue;
            }
            let Some(signal) =
                crate::skill_session::signal::read_signal(self.context.workspace(), &session.token)
            else {
                continue;
            };
            // The token matches, but the run declared output artifacts (a
            // printable, a report, …) that must exist first. Hold the signal —
            // leave the file, keep the tab open — and re-check next tick, so a
            // premature POST can't close the tab before those outputs land. An
            // empty `require` list closes immediately.
            if crate::skill_session::signal::ready_to_close(&signal.require, |p| {
                std::path::Path::new(p).exists()
            }) {
                completed.push((session.id, session.title));
            }
        }
        for (id, title) in exited {
            crate::logging::log(format!("skill session {title}: session exited; closing"));
            self.close_skill_session(id);
        }
        for (id, title) in completed {
            crate::logging::log(format!(
                "skill session {title}: completion signal received; closing"
            ));
            self.close_skill_session(id);
            self.status.set_flash(FlashKind::Info(format!(
                "✓ {} complete",
                title.to_lowercase()
            )));
        }
    }
}
