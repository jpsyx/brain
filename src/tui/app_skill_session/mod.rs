//! Skill-session tabs: the brain-panel tabs that each run one prompt in their
//! own ephemeral session and close themselves when that run is done.
//!
//! Saying "Yes" to the startup daily-triage nudge used to type `/triage` into
//! the *main* brain session, blocking it for the whole (often long, often
//! interactive) pass. Instead we spawn a dedicated, untracked session as an extra
//! tab, seed it with the session's prompt, and let the main session (`Alt+1`)
//! stay free. That is now generic: daily triage is the one builtin definition,
//! and a workspace can declare its own in the `skill_sessions` env array, each
//! getting an identical tab with identical lifecycle.
//!
//! A run is done not when the agent stops talking (it may ask questions
//! mid-pass) but when it POSTs the completion signal to the brain server, using
//! the token and route brain injected and the protocol brain appended to its
//! prompt; `tick_skill_sessions` polls those signals and auto-closes the tab
//! whose token arrives. See [`crate::skill_session`].

use super::*;

mod lifecycle;

use crate::agent::AgentController;
use crate::skill_session::{SkillSessionKey, SkillSessionSpec};

/// One palette row group: a skill session's identity paired with the text the
/// row shows (its `command_label` when starting, its tab `title` when focusing).
pub(crate) type SkillSessionRows = Vec<(SkillSessionKey, String)>;

impl App {
    pub(crate) fn session_done_url_for_port(&self, port: u16) -> String {
        self.context.session_done_url(port)
    }

    /// Whether the brain panel is on screen with *either* the main session or a
    /// skill session (the panel occupies its half whenever one is present).
    pub(crate) fn any_brain_panel_visible(&self) -> bool {
        self.brain.any_panel_visible()
    }

    /// The identities of the open skill-session tabs, in tab order.
    pub(crate) fn skill_session_tab_ids(&self) -> Vec<SessionTabId> {
        self.brain.skill_session_tab_ids()
    }

    /// Which skill sessions are running right now, for the palette's
    /// hide-while-running gate.
    pub(crate) fn running_skill_session_keys(&self) -> Vec<SkillSessionKey> {
        self.brain.running_skill_session_keys()
    }

    /// Every skill session this workspace offers: the builtin daily triage
    /// (only while the daily-triage check is enabled) plus the workspace's own
    /// `skill_sessions` definitions.
    pub(crate) fn available_skill_sessions(&self) -> Vec<SkillSessionSpec> {
        crate::skill_session::available(
            !self.status.daily_triage_check_disabled(),
            self.brain.configured_skill_sessions(),
        )
    }

    /// The palette's two skill-session row groups: the sessions that can be
    /// started now (`command_label` each) and the tabs already open (`title`
    /// each).
    pub(crate) fn skill_session_palette_rows(&self) -> (SkillSessionRows, SkillSessionRows) {
        let available = self.available_skill_sessions();
        let running = self.running_skill_session_keys();
        let runnable = crate::skill_session::runnable(&available, &running)
            .into_iter()
            .map(|spec| (spec.key, spec.command_label.clone()))
            .collect();
        let open = self.brain.skill_session_rows();
        (runnable, open)
    }

    /// The tab actually showable right now: a `Session` only while a tab with
    /// that identity exists, else `Main`.
    pub(crate) fn effective_brain_tab(&self) -> BrainTab {
        self.shell.active_brain_tab(&self.skill_session_tab_ids())
    }

    /// The controller behind the currently-active tab, if any.
    pub(crate) fn active_brain_controller(&self) -> Option<&AgentController> {
        self.brain.active_controller(self.effective_brain_tab())
    }

    /// Mutable counterpart of [`Self::active_brain_controller`] used by the
    /// per-frame terminal resize.
    pub(crate) fn active_brain_controller_mut(&mut self) -> Option<&mut AgentController> {
        let tab = self.effective_brain_tab();
        self.brain.active_controller_mut(tab)
    }

    /// The active tab's title, used by the panel border.
    pub(crate) fn active_brain_tab_title(&self) -> Option<&str> {
        self.brain.active_tab_title(self.effective_brain_tab())
    }

    /// The tab strip's labels, in tab order: the main session first, then each
    /// open skill session's title.
    pub(crate) fn brain_tab_titles(&self) -> Vec<String> {
        self.brain.tab_titles()
    }

    /// Where the active tab sits in the tab strip (`0` = the main session).
    pub(crate) fn active_brain_tab_index(&self) -> usize {
        let ids = self.skill_session_tab_ids();
        self.shell.active_brain_tab_index(&ids)
    }

    /// Select a brain-panel tab (`Alt+1` / `Alt+<n>`) and focus the brain panel.
    /// Selecting a skill session is a no-op when that tab isn't open; selecting
    /// any tab when the panel is closed does nothing.
    pub(crate) fn select_brain_tab(&mut self, tab: BrainTab) -> bool {
        let open = self.skill_session_tab_ids();
        let selected = self
            .shell
            .select_brain_tab(tab, &open, self.any_brain_panel_visible());
        if selected {
            self.status.clear_alert();
        }
        selected
    }

    /// Select a tab by its `Alt+<digit>` slot: slot 0 is the main session, slot
    /// `n` the nth open skill session. Returns whether a tab was actually
    /// selected, so the caller can let an unclaimed keystroke carry on being
    /// ordinary input instead of swallowing it.
    pub(crate) fn select_brain_tab_slot(&mut self, slot: usize) -> bool {
        let ids = self.skill_session_tab_ids();
        let selected = self
            .shell
            .select_brain_tab_slot(slot, &ids, self.any_brain_panel_visible());
        if selected {
            self.status.clear_alert();
        }
        selected
    }

    /// Focus a running skill session by definition (the palette's counterpart to
    /// its `Alt+<n>`). No-op when that session isn't open.
    pub(crate) fn select_skill_session(&mut self, key: SkillSessionKey) {
        if let Some(id) = self.brain.skill_session_id(key) {
            self.select_brain_tab(BrainTab::Session(id));
        }
    }

    /// Cycle the brain-panel tab (`Alt+[` previous / `Alt+]` next) and focus
    /// the panel. With only the main session open this just focuses the panel.
    /// Ordered `[Main, …sessions]` so `next` from Main lands on the first skill
    /// session.
    pub(crate) fn cycle_brain_tab(&mut self, forward: bool) {
        let open = self.skill_session_tab_ids();
        if self
            .shell
            .cycle_brain_tab(&open, forward, self.any_brain_panel_visible())
        {
            self.status.clear_alert();
        }
    }
}
