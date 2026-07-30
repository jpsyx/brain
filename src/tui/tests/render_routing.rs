//! Tests for the wrapped-row offset table and the modal-routing precedence.

use crate::tui::*;

// --- visual_row_offsets / visual_range: logical-line ↔ wrapped-row
//     translation so selection + scroll survive wrapped notes ---

#[test]
fn offsets_are_a_prefix_sum_of_wrapped_heights() {
    // Three single-row lines → rows 0,1,2 and a total of 3.
    assert_eq!(visual_row_offsets(&[1, 1, 1]), vec![0, 1, 2, 3]);
}

#[test]
fn offsets_account_for_a_wrapped_line() {
    // The middle line wraps to 3 visual rows, so every line after it is
    // pushed down by 2 — exactly the drift that broke the highlight.
    assert_eq!(visual_row_offsets(&[1, 3, 1]), vec![0, 1, 4, 5]);
}

#[test]
fn empty_body_has_a_single_zero_offset() {
    assert_eq!(visual_row_offsets(&[]), vec![0]);
}

#[test]
fn visual_range_maps_a_task_past_a_wrapped_line() {
    // offsets for heights [1,3,1]; task at logical line 1 occupies the
    // 3 wrapped rows 1..4, not the logical 1..2.
    let offsets = visual_row_offsets(&[1, 3, 1]);
    assert_eq!(visual_range(&offsets, 1..2), 1..4);
    assert_eq!(visual_range(&offsets, 0..1), 0..1);
    assert_eq!(visual_range(&offsets, 2..3), 4..5);
}

#[test]
fn visual_range_clamps_out_of_bounds_indices() {
    let offsets = visual_row_offsets(&[1, 1]); // [0,1,2]
    assert_eq!(visual_range(&offsets, 5..9), 2..2);
    assert_eq!(visual_range(&[], 0..3), 0..0);
}

// --- modal_input_target: the help modal preempts; the rest keep order ---

fn modals(help: bool, palette: bool, brain_input: bool, confirm: bool) -> ActiveModals {
    ActiveModals {
        help,
        palette,
        brain_input,
        confirm,
        link_picker: false,
    }
}

fn modals_with_picker(link_picker: bool) -> ActiveModals {
    ActiveModals {
        help: false,
        palette: false,
        brain_input: false,
        confirm: false,
        link_picker,
    }
}

#[test]
fn help_modal_preempts_every_other_modal() {
    // Help is captive: while it's up it grabs input over any other modal
    // that happened to be open.
    assert_eq!(
        modal_input_target(modals(true, true, true, true)),
        ModalInput::Help
    );
    assert_eq!(
        modal_input_target(modals(true, false, false, false)),
        ModalInput::Help
    );
}

#[test]
fn without_help_other_modals_keep_their_order() {
    assert_eq!(
        modal_input_target(modals(false, true, true, true)),
        ModalInput::Palette
    );
    assert_eq!(
        modal_input_target(modals(false, false, true, true)),
        ModalInput::BrainInput
    );
    assert_eq!(
        modal_input_target(modals(false, false, false, true)),
        ModalInput::Confirm
    );
}

#[test]
fn no_modal_routes_to_panels() {
    assert_eq!(
        modal_input_target(modals(false, false, false, false)),
        ModalInput::Panels
    );
}

#[test]
fn link_picker_routes_when_no_higher_modal_is_open() {
    assert_eq!(
        modal_input_target(modals_with_picker(true)),
        ModalInput::LinkPicker
    );
    // A higher-precedence modal (e.g. confirm) still wins over the picker.
    assert_eq!(
        modal_input_target(modals(false, false, false, true)),
        ModalInput::Confirm
    );
}
