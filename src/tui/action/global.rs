use crate::skill_session::SkillSessionKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GlobalAction {
    MessageBrain,
    CloseBrain,
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
    ShowMainBrainSession,
    RunSkillSession(SkillSessionKey),
    ShowSkillSession(SkillSessionKey),
}

impl GlobalAction {
    pub(crate) const fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::MessageBrain => Some("^M"),
            Self::CloseBrain => Some("^X"),
            Self::ShowTasks => Some("^T"),
            Self::OpenAgenda => Some("^A"),
            Self::ToggleReceiver
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
            | Self::ShowSkillSession(_) => None,
        }
    }
}
