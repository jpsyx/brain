//! The keybinding table itself: one row per binding, in display order.
//!
//! The `commands` field on each row is the shortcut-parity specification — see
//! the module documentation on [`super`] and the guard tests beside it.

use crate::tasks::view::View;
use crate::tui::action::GlobalAction;
use crate::tui::palette::{Command, EntryCommand, TaskCommand};

use super::{Group, Shortcut};

use Command::{Entry, Global, Task};

/// Every task sub-view jump, for the two bindings that reach all of them.
const TASK_VIEWS: &[Command] = &[
    Global(GlobalAction::ShowTaskView(View::Today)),
    Global(GlobalAction::ShowTaskView(View::Mit)),
    Global(GlobalAction::ShowTaskView(View::PastDue)),
    Global(GlobalAction::ShowTaskView(View::Week)),
    Global(GlobalAction::ShowTaskView(View::Habits)),
    Global(GlobalAction::ShowTaskView(View::Backlog)),
    Global(GlobalAction::ShowTaskView(View::All)),
];

/// Everything the task actions modal offers, since `Enter` is a shortcut *to*
/// that filtered slice of the palette rather than to a single command.
const TASK_ACTIONS: &[Command] = &[
    Task(TaskCommand::Start),
    Task(TaskCommand::MarkComplete),
    Task(TaskCommand::MessageBrainAbout),
    Task(TaskCommand::ToggleNotes),
    Task(TaskCommand::OpenLinks),
    Task(TaskCommand::Remove),
    Task(TaskCommand::Defer(1)),
];

/// Every keybinding, in a stable order. The help modal renders these grouped
/// by [`Group::ORDER`]; the footer renders the `in_footer` subset.
pub(crate) const ALL: &[Shortcut] = &[
    // --- Navigation ---
    Shortcut {
        keys: "j / k",
        label: "task",
        desc: "Next / previous task (accepts a count prefix, e.g. 3j) — cursor movement, not a command",
        group: Group::Navigation,
        in_footer: true,
        commands: &[],
    },
    Shortcut {
        keys: "d / u",
        label: "½-page",
        desc: "Half-page down / up — cursor movement, not a command",
        group: Group::Navigation,
        in_footer: true,
        commands: &[],
    },
    Shortcut {
        keys: "PgDn / PgUp",
        label: "page",
        desc: "Full page down / up — cursor movement, not a command",
        group: Group::Navigation,
        in_footer: false,
        commands: &[],
    },
    Shortcut {
        keys: "g / G",
        label: "first/last",
        desc: "Jump to the first / last task — cursor movement, not a command",
        group: Group::Navigation,
        in_footer: true,
        commands: &[],
    },
    Shortcut {
        keys: "→ / ←",
        label: "notes",
        desc: "Expand / collapse the highlighted entry's notes",
        group: Group::Navigation,
        in_footer: false,
        commands: &[Task(TaskCommand::ToggleNotes)],
    },
    Shortcut {
        keys: "l",
        label: "notes",
        desc: "Toggle the selected entry's notes (preview ↔ full)",
        group: Group::Navigation,
        in_footer: false,
        commands: &[Task(TaskCommand::ToggleNotes)],
    },
    // --- Views ---
    Shortcut {
        keys: "Tab / ⇧Tab",
        label: "view",
        desc: "Cycle the tasks sub-view forward / backward",
        group: Group::Views,
        in_footer: true,
        commands: TASK_VIEWS,
    },
    Shortcut {
        keys: "t m p w h b a",
        label: "jump view",
        desc: "Jump to today / mit / past-due / week / habits / backlog / all",
        group: Group::Views,
        in_footer: false,
        commands: TASK_VIEWS,
    },
    // --- Task actions ---
    Shortcut {
        keys: "↵",
        label: "actions",
        desc: "Open the task actions modal for the selected entry (the palette's task rows, bound to it)",
        group: Group::TaskActions,
        in_footer: true,
        commands: TASK_ACTIONS,
    },
    Shortcut {
        keys: "^D",
        label: "done",
        desc: "Mark the selected task complete (confirm modal)",
        group: Group::TaskActions,
        in_footer: true,
        commands: &[Task(TaskCommand::MarkComplete)],
    },
    Shortcut {
        keys: "^⌫",
        label: "remove",
        desc: "Remove the selected task (confirm modal) — tasks only",
        group: Group::TaskActions,
        in_footer: false,
        commands: &[Task(TaskCommand::Remove)],
    },
    Shortcut {
        keys: "^O",
        label: "links",
        desc: "Open the selected entry's links (Linear + notes URLs)",
        group: Group::TaskActions,
        in_footer: true,
        commands: &[Task(TaskCommand::OpenLinks)],
    },
    Shortcut {
        keys: "r",
        label: "refresh",
        desc: "Reload tasks.csv + habits.csv from disk",
        group: Group::TaskActions,
        in_footer: false,
        commands: &[Global(GlobalAction::ReloadTasks)],
    },
    // --- Brain directory ---
    Shortcut {
        keys: "↵",
        label: "open",
        desc: "Open the highlighted entry (text → editor tab, blob → system open, dir → Finder). On the tree's ../ row it re-roots one level up instead, never above the brain root",
        group: Group::BrainDirectory,
        in_footer: false,
        commands: &[Entry(EntryCommand::Open)],
    },
    Shortcut {
        keys: "^↵",
        label: "reveal",
        desc: "Reveal the highlighted entry's directory in Finder",
        group: Group::BrainDirectory,
        in_footer: false,
        commands: &[Entry(EntryCommand::Reveal)],
    },
    Shortcut {
        keys: "⌥↵",
        label: "explore",
        desc: "Explore the highlighted entry in the directory tree. ⌥↵ again, or Esc, returns to search. Bound to Alt rather than Shift because the kitty keyboard protocol exempts Enter from modifier reporting, so Shift+Enter is byte-identical to Enter",
        group: Group::BrainDirectory,
        in_footer: false,
        commands: &[Entry(EntryCommand::Explore)],
    },
    Shortcut {
        keys: "→ / ← / Space",
        label: "expand",
        desc: "Expand / collapse / toggle the selected tree node. Pure navigation, so it runs no command",
        group: Group::BrainDirectory,
        in_footer: false,
        commands: &[],
    },
    Shortcut {
        keys: "^G",
        label: "pdf",
        desc: "Create a PDF from the highlighted markdown file (green confirm modal)",
        group: Group::BrainDirectory,
        in_footer: false,
        commands: &[Entry(EntryCommand::CreatePdf)],
    },
    Shortcut {
        keys: "^D",
        label: "trash",
        desc: "Move the highlighted entry to the Trash (red confirm modal)",
        group: Group::BrainDirectory,
        in_footer: false,
        commands: &[Entry(EntryCommand::Delete)],
    },
    Shortcut {
        keys: "^R",
        label: "refresh",
        desc: "Re-walk the brain directory, keeping the query",
        group: Group::BrainDirectory,
        in_footer: false,
        commands: &[Global(GlobalAction::RefreshBrainDirectory)],
    },
    // --- Brain ---
    Shortcut {
        keys: "^M",
        label: "brain",
        desc: "Open / focus the brain panel (resumes your latest session)",
        group: Group::Brain,
        in_footer: true,
        commands: &[Global(GlobalAction::MessageBrain)],
    },
    Shortcut {
        keys: "^⇧M",
        label: "brain·task",
        desc: "Message brain about the selected task (hold Shift; needs kitty protocol)",
        group: Group::Brain,
        in_footer: false,
        commands: &[Task(TaskCommand::MessageBrainAbout)],
    },
    Shortcut {
        keys: "^X",
        label: "close session",
        desc: "Close the selected manual or skill session; Main and receiver sessions stay open",
        group: Group::Brain,
        in_footer: false,
        commands: &[Global(GlobalAction::CloseSession)],
    },
    Shortcut {
        keys: "^N",
        label: "new session",
        desc: "Start a new conversation in the selected session (types /new and submits it)",
        group: Group::Brain,
        in_footer: false,
        commands: &[Global(GlobalAction::NewConversation)],
    },
    Shortcut {
        keys: "Alt+H / Alt+L",
        label: "switch",
        desc: "Focus the main view / brain panel (Alt+H always returns to the main view)",
        group: Group::Brain,
        in_footer: false,
        commands: &[
            Global(GlobalAction::FocusMainPanel),
            Global(GlobalAction::FocusBrainPanel),
        ],
    },
    Shortcut {
        keys: "Alt+[ / Alt+]",
        label: "brain tab",
        desc: "Cycle the brain-panel tab (main session ↔ each open session). Reliable everywhere; Alt+1 selects the main session and Alt+<n> the nth session on terminals that support Alt+digit",
        group: Group::Brain,
        in_footer: false,
        commands: &[
            Global(GlobalAction::CycleBrainTab(false)),
            Global(GlobalAction::CycleBrainTab(true)),
        ],
    },
    Shortcut {
        keys: "Alt+U / Alt+D",
        label: "scroll",
        desc: "Scroll the focused panel a half-page up / down — scrolling, not a command",
        group: Group::Brain,
        in_footer: false,
        commands: &[],
    },
    Shortcut {
        keys: "^A",
        label: "agenda",
        desc: "Open today's agenda (offers to generate it when missing)",
        group: Group::Brain,
        in_footer: false,
        commands: &[Global(GlobalAction::OpenAgenda)],
    },
    // --- Search ---
    Shortcut {
        keys: "/",
        label: "search",
        desc: "Enter the tasks view's live fuzzy filter",
        group: Group::Search,
        in_footer: true,
        commands: &[Global(GlobalAction::SearchTasks)],
    },
    Shortcut {
        keys: "Esc",
        label: "clear",
        desc: "Clear the active task filter (quits when none is set)",
        group: Group::Search,
        in_footer: false,
        commands: &[Global(GlobalAction::ClearTaskFilters)],
    },
    // --- Global ---
    Shortcut {
        keys: "Esc",
        label: "dismiss error",
        desc: "Dismiss a pending error banner after any active modal closes — modal input, not a command",
        group: Group::Global,
        in_footer: false,
        commands: &[],
    },
    Shortcut {
        keys: "^L / ^H",
        label: "cycle view",
        desc: "Cycle the main view right / left (tasks ↔ brain directory ↔ logs). Distinct from Alt+H/L, which move panel focus",
        group: Group::Global,
        in_footer: false,
        commands: &[
            Global(GlobalAction::ShowTasks),
            Global(GlobalAction::ShowBrainSearch),
            Global(GlobalAction::ShowBrainLogs),
        ],
    },
    Shortcut {
        keys: "^T / ^B",
        label: "jump view",
        desc: "Jump to the tasks / brain-directory main view",
        group: Group::Global,
        in_footer: false,
        commands: &[
            Global(GlobalAction::ShowTasks),
            Global(GlobalAction::ShowBrainSearch),
        ],
    },
    Shortcut {
        keys: "^P",
        label: "palette",
        desc: "Open the global command palette — every command, from every view",
        group: Group::Global,
        in_footer: true,
        commands: &[],
    },
    Shortcut {
        keys: "Alt+S",
        label: "help",
        desc: "Show all keyboard shortcuts",
        group: Group::Global,
        in_footer: false,
        commands: &[Global(GlobalAction::ShowShortcuts)],
    },
    Shortcut {
        keys: "q",
        label: "quit",
        desc: "Quit the shell (also Ctrl+C) — tasks view normal mode only",
        group: Group::Global,
        in_footer: true,
        commands: &[Global(GlobalAction::Quit)],
    },
    Shortcut {
        keys: "^Q",
        label: "quit",
        desc: "Unconditional quit from either panel — works while the brain panel is focused or a modal is open",
        group: Group::Global,
        in_footer: false,
        commands: &[Global(GlobalAction::Quit)],
    },
];
