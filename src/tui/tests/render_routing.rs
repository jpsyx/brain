//! Tests for the wrapped-row offset table.

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
