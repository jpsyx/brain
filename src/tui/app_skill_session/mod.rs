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
        crate::server::url(
            port,
            &crate::server::session_done_path(self.server_ingress, self.server_local_capability),
        )
    }

    /// Whether the brain panel is on screen with *either* the main session or a
    /// skill session (the panel occupies its half whenever one is present).
    pub(crate) fn any_brain_panel_visible(&self) -> bool {
        self.brain.is_some() || !self.skill_sessions.is_empty()
    }

    /// The identities of the open skill-session tabs, in tab order.
    pub(crate) fn skill_session_tab_ids(&self) -> Vec<SessionTabId> {
        self.skill_sessions.iter().map(|tab| tab.id).collect()
    }

    /// Which skill sessions are running right now, for the palette's
    /// hide-while-running gate.
    pub(crate) fn running_skill_session_keys(&self) -> Vec<SkillSessionKey> {
        self.skill_sessions.iter().map(|tab| tab.key).collect()
    }

    /// Every skill session this workspace offers: the builtin daily triage
    /// (only while the daily-triage check is enabled) plus the workspace's own
    /// `skill_sessions` definitions.
    pub(crate) fn available_skill_sessions(&self) -> Vec<SkillSessionSpec> {
        crate::skill_session::available(
            !self.skip_daily_triage_check,
            self.configured_skill_sessions.as_ref(),
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
        let open = self
            .skill_sessions
            .iter()
            .map(|tab| (tab.key, tab.title.clone()))
            .collect();
        (runnable, open)
    }

    /// The tab actually showable right now: a `Session` only while a tab with
    /// that identity exists, else `Main`.
    pub(crate) fn effective_brain_tab(&self) -> BrainTab {
        resolve_active_tab(self.active_brain_tab, &self.skill_session_tab_ids())
    }

    /// The controller behind the currently-active tab, if any.
    pub(crate) fn active_brain_controller(&self) -> Option<&AgentController> {
        match self.effective_brain_tab() {
            BrainTab::Session(id) => self
                .skill_sessions
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| &tab.controller),
            BrainTab::Main => self.brain.as_ref(),
        }
    }

    /// Mutable counterpart of [`Self::active_brain_controller`] used by the
    /// per-frame terminal resize.
    pub(crate) fn active_brain_controller_mut(&mut self) -> Option<&mut AgentController> {
        match self.effective_brain_tab() {
            BrainTab::Session(id) => self
                .skill_sessions
                .iter_mut()
                .find(|tab| tab.id == id)
                .map(|tab| &mut tab.controller),
            BrainTab::Main => self.brain.as_mut(),
        }
    }

    /// The active tab's title, used by the panel border.
    pub(crate) fn active_brain_tab_title(&self) -> Option<&str> {
        match self.effective_brain_tab() {
            BrainTab::Session(id) => self
                .skill_sessions
                .iter()
                .find(|tab| tab.id == id)
                .map(|tab| tab.title.as_str()),
            BrainTab::Main => None,
        }
    }

    /// The tab strip's labels, in tab order: the main session first, then each
    /// open skill session's title.
    pub(crate) fn brain_tab_titles(&self) -> Vec<String> {
        let mut titles = vec!["Brain".to_owned()];
        titles.extend(self.skill_sessions.iter().map(|tab| tab.title.clone()));
        titles
    }

    /// Where the active tab sits in the tab strip (`0` = the main session).
    pub(crate) fn active_brain_tab_index(&self) -> usize {
        let ids = self.skill_session_tab_ids();
        tab_order(&ids)
            .iter()
            .position(|tab| *tab == self.effective_brain_tab())
            .unwrap_or(0)
    }

    /// Select a brain-panel tab (`Alt+1` / `Alt+<n>`) and focus the brain panel.
    /// Selecting a skill session is a no-op when that tab isn't open; selecting
    /// any tab when the panel is closed does nothing.
    pub(crate) fn select_brain_tab(&mut self, tab: BrainTab) {
        if !self.any_brain_panel_visible() {
            return;
        }
        self.active_brain_tab = resolve_active_tab(tab, &self.skill_session_tab_ids());
        self.focus = Panel::Brain;
        self.alert = None;
    }

    /// Select a tab by its `Alt+<digit>` slot: slot 0 is the main session, slot
    /// `n` the nth open skill session. Returns whether a tab was actually
    /// selected, so the caller can let an unclaimed keystroke carry on being
    /// ordinary input instead of swallowing it.
    pub(crate) fn select_brain_tab_slot(&mut self, slot: usize) -> bool {
        let ids = self.skill_session_tab_ids();
        let Some(tab) = tab_for_slot(slot, &ids) else {
            return false;
        };
        let before = self.effective_brain_tab();
        self.select_brain_tab(tab);
        // `select_brain_tab` no-ops with the panel closed; report that honestly.
        self.effective_brain_tab() == tab && (self.any_brain_panel_visible() || before == tab)
    }

    /// Focus a running skill session by definition (the palette's counterpart to
    /// its `Alt+<n>`). No-op when that session isn't open.
    pub(crate) fn select_skill_session(&mut self, key: SkillSessionKey) {
        if let Some(id) = self
            .skill_sessions
            .iter()
            .find(|tab| tab.key == key)
            .map(|tab| tab.id)
        {
            self.select_brain_tab(BrainTab::Session(id));
        }
    }

    /// Cycle the brain-panel tab (`Alt+[` previous / `Alt+]` next) and focus
    /// the panel. With only the main session open this just focuses the panel.
    /// Ordered `[Main, …sessions]` so `next` from Main lands on the first skill
    /// session.
    pub(crate) fn cycle_brain_tab(&mut self, forward: bool) {
        if !self.any_brain_panel_visible() {
            return;
        }
        let tabs = tab_order(&self.skill_session_tab_ids());
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
}

/// Which tab is actually showable: a `Session` only when a tab with that
/// identity is open, else `Main`. Keeps `Alt+<n>` a no-op for a tab that isn't
/// there and stops rendering / routing from ever pointing at a closed session.
/// Pure.
pub(crate) fn resolve_active_tab(requested: BrainTab, open: &[SessionTabId]) -> BrainTab {
    match requested {
        BrainTab::Session(id) if open.contains(&id) => BrainTab::Session(id),
        _ => BrainTab::Main,
    }
}

/// The tab strip's order: the main session, then each open skill session in the
/// order it was opened. Pure; drives both the `Alt+<digit>` slots and the
/// `Alt+[` / `Alt+]` cycle so the two can't disagree.
pub(crate) fn tab_order(open: &[SessionTabId]) -> Vec<BrainTab> {
    let mut tabs = vec![BrainTab::Main];
    tabs.extend(open.iter().copied().map(BrainTab::Session));
    tabs
}

/// The tab an `Alt+<digit>` slot selects: slot 0 (`Alt+1`) is the main session,
/// slot `n` the nth open skill session. `None` when nothing occupies that slot.
/// Pure.
pub(crate) fn tab_for_slot(slot: usize, open: &[SessionTabId]) -> Option<BrainTab> {
    tab_order(open).get(slot).copied()
}

#[cfg(test)]
mod tests {
    use super::{resolve_active_tab, tab_for_slot, tab_order};
    use crate::tui::{BrainTab, SessionTabId};

    const FIRST: SessionTabId = SessionTabId(0);
    const SECOND: SessionTabId = SessionTabId(1);

    #[test]
    fn a_session_tab_is_shown_only_while_it_is_open() {
        assert_eq!(
            resolve_active_tab(BrainTab::Session(FIRST), &[FIRST]),
            BrainTab::Session(FIRST)
        );
        assert_eq!(
            resolve_active_tab(BrainTab::Session(FIRST), &[SECOND]),
            BrainTab::Main
        );
        assert_eq!(
            resolve_active_tab(BrainTab::Session(FIRST), &[]),
            BrainTab::Main
        );
    }

    #[test]
    fn main_stays_main_regardless_of_open_sessions() {
        assert_eq!(resolve_active_tab(BrainTab::Main, &[FIRST]), BrainTab::Main);
        assert_eq!(resolve_active_tab(BrainTab::Main, &[]), BrainTab::Main);
    }

    #[test]
    fn the_tab_strip_leads_with_the_main_session() {
        assert_eq!(tab_order(&[]), vec![BrainTab::Main]);
        assert_eq!(
            tab_order(&[FIRST, SECOND]),
            vec![
                BrainTab::Main,
                BrainTab::Session(FIRST),
                BrainTab::Session(SECOND)
            ]
        );
    }

    #[test]
    fn alt_digit_slots_count_from_the_main_session() {
        assert_eq!(tab_for_slot(0, &[FIRST]), Some(BrainTab::Main));
        assert_eq!(
            tab_for_slot(1, &[FIRST, SECOND]),
            Some(BrainTab::Session(FIRST))
        );
        assert_eq!(
            tab_for_slot(2, &[FIRST, SECOND]),
            Some(BrainTab::Session(SECOND))
        );
        // A digit past the last open tab does nothing rather than wrapping.
        assert_eq!(tab_for_slot(2, &[FIRST]), None);
        assert_eq!(tab_for_slot(1, &[]), None);
    }
}
