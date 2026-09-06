use super::SessionTabId;
use crate::skill_session::SkillSessionKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GlobalAction {
    MessageBrain,
    StartManualSession,
    ShowMainBrainSession,
    ShowSessionTab(SessionTabId),
    CloseSessionTab(SessionTabId),
    ToggleReceiver,
    ToggleLayout,
    ShowTasks,
    ShowReceiverServerStatus,
    ShowReceiverServerLogs,
    ShowBrainLogs,
    OpenHabits,
    SyncBrainNow,
    ShowSyncStatus,
    OpenAgenda,
    ToggleDailyTriageAlert,
    RunSkillSession(SkillSessionKey),
}

impl GlobalAction {
    pub(crate) const fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::MessageBrain => Some("^M"),
            Self::ShowTasks => Some("^T"),
            Self::OpenAgenda => Some("^A"),
            Self::StartManualSession
            | Self::ToggleReceiver
            | Self::ToggleLayout
            | Self::ShowReceiverServerStatus
            | Self::ShowReceiverServerLogs
            | Self::ShowBrainLogs
            | Self::OpenHabits
            | Self::SyncBrainNow
            | Self::ShowSyncStatus
            | Self::ToggleDailyTriageAlert
            | Self::ShowMainBrainSession
            | Self::RunSkillSession(_)
            | Self::ShowSessionTab(_)
            | Self::CloseSessionTab(_) => None,
        }
    }
}
