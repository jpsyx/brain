use crate::agent::AgentController;
use crate::manual_session::{ManualSessionId, ManualSessionRecord};
use crate::tui::app_brain::launch::arrival::{ExitedPanel, SessionStartup};
use crate::tui::model::SessionTabId;

use super::{
    ManualSessionObservation, ManualSessionTabIdExhausted, ManualTabMetadata, RemovedManualSession,
    SessionPaletteEntry, SessionTabIdExhausted, SessionTabKind, SessionTabs,
};

impl SessionTabs {
    pub(in crate::tui::state::brain) fn user_session_rows(&self) -> Vec<SessionPaletteEntry> {
        self.tabs
            .iter()
            .filter(|tab| {
                matches!(
                    tab.metadata,
                    SessionTabKind::Manual(_) | SessionTabKind::Skill(_)
                )
            })
            .map(|tab| SessionPaletteEntry::new(tab.id, &tab.title))
            .collect()
    }

    pub(in crate::tui::state::brain) fn manual_session_rows(&self) -> Vec<SessionPaletteEntry> {
        self.tabs
            .iter()
            .filter(|tab| matches!(tab.metadata, SessionTabKind::Manual(_)))
            .map(|tab| SessionPaletteEntry::new(tab.id, &tab.title))
            .collect()
    }

    pub(in crate::tui::state::brain) fn add_manual_session(
        &mut self,
        record: ManualSessionRecord,
        controller: AgentController,
        resumed_session_id: Option<String>,
    ) -> Result<SessionTabId, ManualSessionTabIdExhausted> {
        self.add(
            record.name.as_str().to_owned(),
            SessionTabKind::Manual(ManualTabMetadata {
                id: record.id,
                resumed_session_id,
                startup: SessionStartup::default(),
            }),
            controller,
        )
        .map_err(|SessionTabIdExhausted| ManualSessionTabIdExhausted)
    }

    pub(in crate::tui::state::brain) fn remove_manual_session(
        &mut self,
        id: SessionTabId,
    ) -> Option<RemovedManualSession> {
        let index = self
            .tabs
            .iter()
            .position(|tab| tab.id == id && matches!(tab.metadata, SessionTabKind::Manual(_)))?;
        let mut tab = self.tabs.remove(index);
        let _ = tab.controller.shutdown();
        let SessionTabKind::Manual(manual) = tab.metadata else {
            unreachable!("the located tab was a manual session")
        };
        Some(RemovedManualSession { id: manual.id })
    }

    pub(in crate::tui::state::brain) fn replace_manual_controller(
        &mut self,
        id: SessionTabId,
        record: &ManualSessionRecord,
        mut controller: AgentController,
        resumed_session_id: Option<String>,
    ) -> anyhow::Result<()> {
        let Some(tab) = self.tabs.iter_mut().find(|tab| {
            tab.id == id
                && matches!(&tab.metadata, SessionTabKind::Manual(manual) if manual.id == record.id)
        }) else {
            let _ = controller.shutdown();
            anyhow::bail!("manual tab identity changed");
        };
        if let Err(error) = tab.controller.shutdown() {
            let _ = controller.shutdown();
            return Err(error.into());
        }
        tab.controller = controller;
        tab.metadata = SessionTabKind::Manual(ManualTabMetadata {
            id: record.id.clone(),
            resumed_session_id,
            startup: SessionStartup::default(),
        });
        Ok(())
    }

    pub(in crate::tui::state::brain) fn manual_session_id(
        &self,
        id: SessionTabId,
    ) -> Option<&ManualSessionId> {
        self.tabs.iter().find_map(|tab| match &tab.metadata {
            SessionTabKind::Manual(manual) if tab.id == id => Some(&manual.id),
            SessionTabKind::Manual(_) | SessionTabKind::Skill(_) | SessionTabKind::Receiver(_) => {
                None
            }
        })
    }

    pub(in crate::tui::state::brain) fn record_manual_restore_failed(&mut self, id: SessionTabId) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
            && let SessionTabKind::Manual(manual) = &mut tab.metadata
        {
            manual.startup = SessionStartup::Failed;
        }
    }

    pub(in crate::tui::state::brain) fn arm_manual_startup(
        &mut self,
        id: SessionTabId,
        now: std::time::Instant,
    ) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
            && let SessionTabKind::Manual(manual) = &mut tab.metadata
        {
            manual.startup = SessionStartup::starting(now);
        }
    }

    pub(in crate::tui::state::brain) fn manual_exit_decision(
        &mut self,
        observation: &ManualSessionObservation,
        now: std::time::Instant,
    ) -> Option<ExitedPanel> {
        let tab = self.tabs.iter_mut().find(|tab| tab.id == observation.id)?;
        let SessionTabKind::Manual(manual) = &mut tab.metadata else {
            return None;
        };
        manual.startup.observe(
            observation.alive?,
            observation.resumed_session_id.as_deref(),
            now,
        )
    }

    pub(in crate::tui::state::brain) fn manual_session_observations(
        &self,
    ) -> Vec<ManualSessionObservation> {
        self.tabs
            .iter()
            .filter_map(|tab| match &tab.metadata {
                SessionTabKind::Manual(manual) => Some(ManualSessionObservation {
                    id: tab.id,
                    manual_session_id: manual.id.clone(),
                    resumed_session_id: manual.resumed_session_id.clone(),
                    alive: tab.controller.is_alive().ok(),
                }),
                SessionTabKind::Skill(_) | SessionTabKind::Receiver(_) => None,
            })
            .collect()
    }
}
