//! Modal key routing: which overlay (if any) should consume the next
//! keystroke, resolved in a fixed precedence order before any panel / chord /
//! leader handling so an open modal is fully captive.

use crate::tui::*;

/// Which overlay should consume the next keystroke. Resolved before any
/// panel / chord / leader handling so an open modal is fully captive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalInput {
    Help,
    Palette,
    BrainInput,
    Confirm,
    LinkPicker,
    /// No modal is up — route to the panels (tasks / brain).
    Panels,
}

/// Which overlays are currently open. Mutually exclusive in practice.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ActiveModals {
    pub(crate) help: bool,
    pub(crate) palette: bool,
    pub(crate) brain_input: bool,
    pub(crate) confirm: bool,
    pub(crate) link_picker: bool,
}

/// Decide the keystroke target from the active overlays, in a fixed
/// precedence order.
pub(crate) const fn modal_input_target(m: ActiveModals) -> ModalInput {
    if m.help {
        ModalInput::Help
    } else if m.palette {
        ModalInput::Palette
    } else if m.brain_input {
        ModalInput::BrainInput
    } else if m.confirm {
        ModalInput::Confirm
    } else if m.link_picker {
        ModalInput::LinkPicker
    } else {
        ModalInput::Panels
    }
}

/// Route a keystroke to whichever modal overlay is active. Returns `true`
/// when a modal consumed the key (the caller should skip panel handling).
pub(crate) fn route_modal_key(app: &mut App<'_>, k: &crossterm::event::KeyEvent, ctrl: bool) -> bool {
    let target = modal_input_target(ActiveModals {
        help: app.help.is_some(),
        palette: app.palette.is_some(),
        brain_input: app.brain_input.is_some(),
        confirm: app.confirm.is_some(),
        link_picker: app.link_picker.is_some(),
    });
    match target {
        ModalInput::Help => handle_help_key(app, k, ctrl),
        ModalInput::Palette => handle_palette_key(app, k, ctrl),
        ModalInput::BrainInput => handle_brain_input_key(app, k, ctrl),
        ModalInput::Confirm => handle_confirm_key(app, k, ctrl),
        ModalInput::LinkPicker => handle_link_picker_key(app, k, ctrl),
        ModalInput::Panels => return false,
    }
    true
}
