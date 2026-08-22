//! Modal key routing. The same data-bearing enum used by drawing selects the
//! captive input handler.

use crate::tui::App;
use crate::tui::handlers::{
    handle_assignee_filter_key, handle_brain_input_key, handle_confirm_key, handle_help_key,
    handle_link_picker_key, handle_palette_key, handle_sync_log_key,
};
use crate::tui::overlay::{ModalInput, modal_input_target};
use crate::tui::search_view::{route_search_confirm, route_search_palette};

/// Route a keystroke to the active modal. Returns `true` when an overlay
/// consumed the key, so the caller skips panel handling.
pub(crate) fn route_modal_key(app: &mut App, k: &crossterm::event::KeyEvent, ctrl: bool) -> bool {
    match modal_input_target(app.overlay.as_ref()) {
        ModalInput::Help => handle_help_key(app, k, ctrl),
        ModalInput::SyncLog => handle_sync_log_key(app, k),
        ModalInput::TaskPalette => handle_palette_key(app, k, ctrl),
        ModalInput::BrainInput => handle_brain_input_key(app, k, ctrl),
        ModalInput::TaskConfirmation => handle_confirm_key(app, k, ctrl),
        ModalInput::SearchPalette => route_search_palette(app, k),
        ModalInput::SearchConfirmation => route_search_confirm(app, k),
        ModalInput::LinkPicker => handle_link_picker_key(app, k, ctrl),
        ModalInput::AssigneeFilter => handle_assignee_filter_key(app, k, ctrl),
        ModalInput::Panels => return false,
    }
    true
}
