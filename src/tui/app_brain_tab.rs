//! Shared brain-panel tab observation and user navigation.

use crate::agent::AgentController;
use crate::tui::App;
use crate::tui::model::BrainTab;

impl App {
    /// The tab actually showable right now: an ephemeral tab only while its
    /// stable identity exists, otherwise the main session.
    pub(crate) fn effective_brain_tab(&self) -> BrainTab {
        self.shell.active_brain_tab(&self.brain.ephemeral_tab_ids())
    }

    pub(crate) fn active_brain_controller(&self) -> Option<&AgentController> {
        let tab = self.shell.active_brain_tab(&self.brain.ephemeral_tab_ids());
        self.brain.active_controller(tab)
    }

    pub(crate) fn active_brain_controller_mut(&mut self) -> Option<&mut AgentController> {
        let tab = self.effective_brain_tab();
        self.brain.active_controller_mut(tab)
    }

    pub(crate) fn active_brain_tab_title(&self) -> Option<&str> {
        let tab = self.shell.active_brain_tab(&self.brain.ephemeral_tab_ids());
        self.brain.active_tab_title(tab)
    }

    pub(crate) fn active_brain_tab_index(&self) -> usize {
        let ids = self.brain.ephemeral_tab_ids();
        self.shell.active_brain_tab_index(&ids)
    }

    /// Select a showable brain-panel tab and focus the panel. A receiver-run
    /// insertion never calls this method.
    pub(crate) fn select_brain_tab(&mut self, tab: BrainTab) -> bool {
        let open = self.brain.ephemeral_tab_ids();
        let selected = self
            .shell
            .select_brain_tab(tab, &open, self.brain.any_panel_visible());
        if selected {
            self.status.clear_alert();
        }
        selected
    }

    /// Select slot zero for the main session or a later slot for the matching
    /// ephemeral tab in shared insertion order.
    pub(crate) fn select_brain_tab_slot(&mut self, slot: usize) -> bool {
        let ids = self.brain.ephemeral_tab_ids();
        let selected = self
            .shell
            .select_brain_tab_slot(slot, &ids, self.brain.any_panel_visible());
        if selected {
            self.status.clear_alert();
        }
        selected
    }

    pub(crate) fn cycle_brain_tab(&mut self, forward: bool) {
        let open = self.brain.ephemeral_tab_ids();
        if self
            .shell
            .cycle_brain_tab(&open, forward, self.brain.any_panel_visible())
        {
            self.status.clear_alert();
        }
    }
}
