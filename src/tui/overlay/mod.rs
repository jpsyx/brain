//! The shell's single modal owner and its explicit state transitions.

use crate::confirm::Confirm;
use crate::tui::modal_state::{
    AssigneeFilterState, BrainInputState, CaptureNoteState, ConfirmState, HelpState,
    LinkPickerState, ManualSessionRenameState, SessionClosePickerState, SessionRenamePickerState,
    SyncLogState,
};
use crate::tui::palette::{CommandPaletteState, EntryTargetPicker, TaskTargetPicker};

/// The only modal state the shell can represent. Each variant owns exactly the
/// data its input and draw routes need.
pub(crate) enum Overlay {
    /// The global command palette *and* the task actions modal: one state,
    /// which of the two is open is a flag on it.
    CommandPalette(CommandPaletteState),
    /// A task command asking which task to run on.
    TaskTargetPicker(TaskTargetPicker),
    /// An entry command asking which file or directory to run on.
    EntryTargetPicker(EntryTargetPicker),
    BrainInput(BrainInputState),
    /// Naming a new note for the `capture/` in-basket.
    CaptureNote(CaptureNoteState),
    ManualSessionRename(ManualSessionRenameState),
    SessionClosePicker(SessionClosePickerState),
    SessionRenamePicker(SessionRenamePickerState),
    TaskConfirmation(ConfirmState),
    SearchConfirmation(Confirm),
    LinkPicker(LinkPickerState),
    AssigneeFilter(AssigneeFilterState),
    Help(HelpState),
    SyncLog(SyncLogState),
}

impl Overlay {
    /// The link-picker's highlighted URL, when a link picker is what's open.
    pub(crate) fn picked_link_url(&self) -> Option<&str> {
        match self {
            Self::LinkPicker(picker) => picker.selected_url(),
            _ => None,
        }
    }

    /// The confirm modal's focused button, when a confirm modal is open.
    pub(crate) fn confirm_focus(&self) -> Option<crate::tui::modal_state::ConfirmChoice> {
        match self {
            Self::TaskConfirmation(confirm) => Some(confirm.focus),
            _ => None,
        }
    }
}

/// Input destination derived from the active overlay variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalInput {
    CommandPalette,
    TaskTargetPicker,
    EntryTargetPicker,
    BrainInput,
    CaptureNote,
    ManualSessionRename,
    SessionClosePicker,
    SessionRenamePicker,
    TaskConfirmation,
    SearchConfirmation,
    LinkPicker,
    AssigneeFilter,
    Help,
    SyncLog,
    Panels,
}

pub(crate) const fn modal_input_target(active: Option<&Overlay>) -> ModalInput {
    match active {
        Some(Overlay::CommandPalette(_)) => ModalInput::CommandPalette,
        Some(Overlay::TaskTargetPicker(_)) => ModalInput::TaskTargetPicker,
        Some(Overlay::EntryTargetPicker(_)) => ModalInput::EntryTargetPicker,
        Some(Overlay::BrainInput(_)) => ModalInput::BrainInput,
        Some(Overlay::CaptureNote(_)) => ModalInput::CaptureNote,
        Some(Overlay::ManualSessionRename(_)) => ModalInput::ManualSessionRename,
        Some(Overlay::SessionClosePicker(_)) => ModalInput::SessionClosePicker,
        Some(Overlay::SessionRenamePicker(_)) => ModalInput::SessionRenamePicker,
        Some(Overlay::TaskConfirmation(_)) => ModalInput::TaskConfirmation,
        Some(Overlay::SearchConfirmation(_)) => ModalInput::SearchConfirmation,
        Some(Overlay::LinkPicker(_)) => ModalInput::LinkPicker,
        Some(Overlay::AssigneeFilter(_)) => ModalInput::AssigneeFilter,
        Some(Overlay::Help(_)) => ModalInput::Help,
        Some(Overlay::SyncLog(_)) => ModalInput::SyncLog,
        None => ModalInput::Panels,
    }
}

/// Open `next` only when no modal is active.
pub(crate) fn open_overlay(active: &mut Option<Overlay>, next: Overlay) -> bool {
    if active.is_some() {
        return false;
    }
    *active = Some(next);
    true
}

/// Replace the active modal as one explicit transition.
pub(crate) fn replace_overlay(active: &mut Option<Overlay>, next: Overlay) -> Option<Overlay> {
    active.replace(next)
}

/// Close and return the active modal.
pub(crate) fn close_overlay(active: &mut Option<Overlay>) -> Option<Overlay> {
    active.take()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::confirm::Confirm;
    use crate::tui::modal_state::{
        AssigneeFilterState, BrainInputState, CaptureNoteState, ConfirmState, HelpState,
        LinkPickerState, ManualSessionRenameState, SessionClosePickerState,
        SessionRenamePickerState, SyncLogState,
    };
    use crate::tui::model::SessionTabId;
    use crate::tui::overlay::{
        ModalInput, Overlay, close_overlay, modal_input_target, open_overlay, replace_overlay,
    };
    use crate::tui::palette::{
        CommandPaletteState, EntryCommand, EntryTargetPicker, PaletteContext, TaskCommand,
        TaskTargetPicker,
    };

    fn command_palette() -> Overlay {
        Overlay::CommandPalette(CommandPaletteState::new(&PaletteContext::default()))
    }

    #[test]
    fn opening_populates_an_empty_overlay_slot() {
        let mut active = None;

        assert!(open_overlay(
            &mut active,
            Overlay::Help(HelpState { scroll: 4 })
        ));
        assert!(matches!(
            active,
            Some(Overlay::Help(HelpState { scroll: 4 }))
        ));
    }

    #[test]
    fn opening_does_not_overwrite_an_active_overlay() {
        let mut active = Some(Overlay::Help(HelpState { scroll: 4 }));

        assert!(!open_overlay(
            &mut active,
            Overlay::SyncLog(SyncLogState { scroll: 8 })
        ));
        assert!(matches!(
            active,
            Some(Overlay::Help(HelpState { scroll: 4 }))
        ));
    }

    #[test]
    fn replacing_returns_the_displaced_overlay() {
        let mut active = Some(Overlay::Help(HelpState { scroll: 4 }));

        let previous = replace_overlay(&mut active, Overlay::SyncLog(SyncLogState { scroll: 8 }));

        assert!(matches!(
            previous,
            Some(Overlay::Help(HelpState { scroll: 4 }))
        ));
        assert!(matches!(
            active,
            Some(Overlay::SyncLog(SyncLogState { scroll: 8 }))
        ));
    }

    #[test]
    fn closing_returns_the_active_overlay_and_leaves_none() {
        let mut active = Some(Overlay::Help(HelpState { scroll: 4 }));

        let closed = close_overlay(&mut active);

        assert!(matches!(
            closed,
            Some(Overlay::Help(HelpState { scroll: 4 }))
        ));
        assert!(active.is_none());
    }

    #[test]
    fn every_data_bearing_variant_routes_by_its_enum_identity() {
        let cases = [
            (command_palette(), ModalInput::CommandPalette),
            (
                Overlay::TaskTargetPicker(TaskTargetPicker::new(
                    TaskCommand::MarkComplete,
                    Vec::new(),
                )),
                ModalInput::TaskTargetPicker,
            ),
            (
                Overlay::EntryTargetPicker(EntryTargetPicker::new(
                    EntryCommand::Delete,
                    crate::picker::App::new(&[], ""),
                )),
                ModalInput::EntryTargetPicker,
            ),
            (
                Overlay::BrainInput(BrainInputState::about("T1".to_owned(), "Task".to_owned())),
                ModalInput::BrainInput,
            ),
            (
                Overlay::ManualSessionRename(ManualSessionRenameState::rename(
                    SessionTabId(1),
                    "Atlas",
                )),
                ModalInput::ManualSessionRename,
            ),
            (
                Overlay::CaptureNote(CaptureNoteState::new("2026-09-23:14-05".to_owned())),
                ModalInput::CaptureNote,
            ),
            (
                Overlay::SessionClosePicker(SessionClosePickerState::new(Vec::new())),
                ModalInput::SessionClosePicker,
            ),
            (
                Overlay::SessionRenamePicker(SessionRenamePickerState::new(Vec::new())),
                ModalInput::SessionRenamePicker,
            ),
            (
                Overlay::TaskConfirmation(ConfirmState::generate_agenda()),
                ModalInput::TaskConfirmation,
            ),
            (
                Overlay::SearchConfirmation(Confirm::pdf(PathBuf::from("plan.md"))),
                ModalInput::SearchConfirmation,
            ),
            (
                Overlay::LinkPicker(LinkPickerState::new("T1".to_owned(), Vec::new())),
                ModalInput::LinkPicker,
            ),
            (
                Overlay::AssigneeFilter(AssigneeFilterState::new(&[], None)),
                ModalInput::AssigneeFilter,
            ),
            (Overlay::Help(HelpState { scroll: 0 }), ModalInput::Help),
            (
                Overlay::SyncLog(SyncLogState { scroll: 0 }),
                ModalInput::SyncLog,
            ),
        ];

        for (overlay, expected) in cases {
            assert_eq!(modal_input_target(Some(&overlay)), expected);
        }
        assert_eq!(modal_input_target(None), ModalInput::Panels);
    }
}
