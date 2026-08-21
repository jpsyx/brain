//! The shell's single modal owner and its explicit state transitions.

use crate::confirm::Confirm;
use crate::menu::MenuApp;
use crate::tui::{
    AssigneeFilterState, BrainInputState, ConfirmState, HelpState, LinkPickerState, PaletteState,
    SyncLogState,
};

/// The only modal state the shell can represent. Each variant owns exactly the
/// data its input and draw routes need.
pub(crate) enum Overlay {
    TaskPalette(PaletteState),
    BrainInput(BrainInputState),
    TaskConfirmation(ConfirmState),
    SearchPalette(MenuApp),
    SearchConfirmation(Confirm),
    LinkPicker(LinkPickerState),
    AssigneeFilter(AssigneeFilterState),
    Help(HelpState),
    SyncLog(SyncLogState),
}

/// Input destination derived from the active overlay variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalInput {
    TaskPalette,
    BrainInput,
    TaskConfirmation,
    SearchPalette,
    SearchConfirmation,
    LinkPicker,
    AssigneeFilter,
    Help,
    SyncLog,
    Panels,
}

pub(crate) const fn modal_input_target(active: Option<&Overlay>) -> ModalInput {
    match active {
        Some(Overlay::TaskPalette(_)) => ModalInput::TaskPalette,
        Some(Overlay::BrainInput(_)) => ModalInput::BrainInput,
        Some(Overlay::TaskConfirmation(_)) => ModalInput::TaskConfirmation,
        Some(Overlay::SearchPalette(_)) => ModalInput::SearchPalette,
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
    use crate::menu::{MenuApp, Targets};
    use crate::state::PanelSide;
    use crate::tui::{
        AssigneeFilterState, BrainInputState, ConfirmState, HelpState, LinkKind, LinkPickerState,
        ModalInput, Overlay, PaletteState, SyncLogState, close_overlay, modal_input_target,
        open_overlay, replace_overlay,
    };

    fn task_palette() -> Overlay {
        Overlay::TaskPalette(PaletteState::new(
            None,
            false,
            false,
            false,
            LinkKind::None,
            false,
            false,
        ))
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
            (task_palette(), ModalInput::TaskPalette),
            (
                Overlay::BrainInput(BrainInputState::about("T1".to_owned(), "Task".to_owned())),
                ModalInput::BrainInput,
            ),
            (
                Overlay::TaskConfirmation(ConfirmState::generate_agenda()),
                ModalInput::TaskConfirmation,
            ),
            (
                Overlay::SearchPalette(MenuApp::new(PanelSide::Right, true, &Targets::default())),
                ModalInput::SearchPalette,
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
