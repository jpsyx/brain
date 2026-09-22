use super::SessionTabId;
use crate::entry::Bucket;
use crate::skill_session::SkillSessionKey;
use crate::tasks::view::View;

/// Every action the shell can take that needs no target beyond the app itself.
///
/// One variant per user-visible command, including the ones that also carry a
/// direct keyboard shortcut — the command palette is the parent set of every
/// action, so a shortcut with no `GlobalAction` (or task / entry command)
/// behind it would be unreachable from the palette. See the shortcut-parity
/// rule in `AGENTS.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GlobalAction {
    MessageBrain,
    StartManualSession,
    RenameSession,
    CloseSession,
    /// Run the selected adapter's semantic "new conversation" sequence in the
    /// live session tab (`Ctrl+N`).
    NewConversation,
    ShowMainBrainSession,
    ShowSessionTab(SessionTabId),
    /// Step the brain-panel tab strip: `true` is the next tab (`Alt+]`),
    /// `false` the previous one (`Alt+[`).
    CycleBrainTab(bool),
    /// Move keyboard focus to the brain panel (`Alt+L`).
    FocusBrainPanel,
    /// Move keyboard focus back to the main view (`Alt+H`).
    FocusMainPanel,
    ToggleReceiver,
    ToggleLayout,
    ShowTasks,
    /// Show the brain-directory (fuzzy search) main view (`Ctrl+B`).
    ShowBrainSearch,
    /// Open the brain-directory tree at the brain root, collapsed (`Ctrl+E`).
    /// Unlike [`EntryCommand::Explore`](crate::tui::palette::EntryCommand) it
    /// needs no target, which is why it is a global action.
    OpenFileExplorer,
    /// Flip whether the tree sub-view shows dotted names (`.`).
    ToggleHiddenFiles,
    ShowReceiverServerStatus,
    ShowReceiverServerLogs,
    ShowBrainLogs,
    OpenHabits,
    SyncBrainNow,
    ShowSyncStatus,
    OpenAgenda,
    ToggleDailyTriageAlert,
    RunSkillSession(SkillSessionKey),
    /// Ask the brain agent to collect a new task, preserving actor assignment
    /// as the default unless the user explicitly selects another member.
    AddTask,
    /// Open the native portable-member picker that filters the current view.
    ChooseAssigneeFilter,
    /// Drop the tasks view's text query and assignee filter (`Esc`).
    ClearTaskFilters,
    /// Enter the tasks view's live fuzzy filter (`/`).
    SearchTasks,
    /// Re-read `tasks.csv` + `habits.csv` from disk (`r`).
    ReloadTasks,
    /// Jump the tasks view to one of its named sub-views (`t m p w h b a`).
    ShowTaskView(View),
    /// Re-walk the brain directory, keeping the query (`Ctrl+R`).
    RefreshBrainDirectory,
    /// Rescope the brain-directory search to a single bucket.
    SearchBucket(Bucket),
    /// Restore the brain-directory search to every bucket.
    SearchEverything,
    /// Open the keyboard-shortcuts help modal (`Alt+S`).
    ShowShortcuts,
    /// Leave the shell (`Ctrl+Q`).
    Quit,
}

impl GlobalAction {
    /// The direct keystroke that fires this action without the palette,
    /// rendered as the dim `[…]` hint next to its row. `None` when the action
    /// is palette-only.
    pub(crate) const fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::MessageBrain => Some("^M"),
            Self::CloseSession => Some("^X"),
            Self::NewConversation => Some("^N"),
            Self::CycleBrainTab(true) => Some("⌥]"),
            Self::CycleBrainTab(false) => Some("⌥["),
            Self::FocusBrainPanel => Some("⌥L"),
            Self::FocusMainPanel => Some("⌥H"),
            Self::ShowTasks => Some("^T"),
            Self::ShowBrainSearch => Some("^B"),
            Self::OpenFileExplorer => Some("^E"),
            Self::ToggleHiddenFiles => Some("."),
            Self::OpenAgenda => Some("^A"),
            Self::ClearTaskFilters => Some("Esc"),
            Self::SearchTasks => Some("/"),
            Self::ReloadTasks => Some("r"),
            Self::ShowTaskView(view) => Some(view_shortcut_hint(view)),
            Self::RefreshBrainDirectory => Some("^R"),
            Self::ShowShortcuts => Some("⌥S"),
            Self::Quit => Some("^Q"),
            Self::StartManualSession
            | Self::RenameSession
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
            | Self::AddTask
            | Self::ChooseAssigneeFilter
            | Self::SearchBucket(_)
            | Self::SearchEverything => None,
        }
    }
}

/// The bare letter that jumps to a tasks sub-view, mirroring `view_shortcut`
/// in `tui::keymap`.
const fn view_shortcut_hint(view: View) -> &'static str {
    match view {
        View::Today => "t",
        View::Mit => "m",
        View::PastDue => "p",
        View::Week => "w",
        View::Habits => "h",
        View::Backlog => "b",
        View::All => "a",
    }
}
