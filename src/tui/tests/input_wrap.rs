//! Tests for brain-input soft wrapping (wrap_input).

use crate::tui::*;

// --- wrap_input (brain-input soft wrapping) ---

#[test]
fn wrap_input_empty_is_one_blank_row() {
    assert_eq!(wrap_input("", 10), vec![String::new()]);
}

#[test]
fn wrap_input_short_text_is_one_row() {
    assert_eq!(wrap_input("hello", 10), vec!["hello".to_owned()]);
}

#[test]
fn wrap_input_breaks_on_word_boundary() {
    // "hello world" at width 8 breaks after "hello " (the space stays
    // on the first row), not mid-word.
    assert_eq!(
        wrap_input("hello world", 8),
        vec!["hello ".to_owned(), "world".to_owned()]
    );
}

#[test]
fn wrap_input_hard_splits_overlong_word() {
    // No break opportunity within the window → hard break at width.
    assert_eq!(
        wrap_input("abcdefghij", 4),
        vec!["abcd".to_owned(), "efgh".to_owned(), "ij".to_owned()]
    );
}

#[test]
fn wrap_input_honors_explicit_newlines() {
    assert_eq!(
        wrap_input("a\nb", 10),
        vec!["a".to_owned(), "b".to_owned()]
    );
}

#[test]
fn wrap_input_trailing_newline_yields_trailing_blank_row() {
    // A trailing newline leaves an empty last row so the cursor lands
    // at the start of a fresh line.
    assert_eq!(
        wrap_input("hi\n", 10),
        vec!["hi".to_owned(), String::new()]
    );
}

#[test]
fn wrap_input_preserves_every_character() {
    let text = "the quick brown fox jumps over the lazy dog";
    let joined: String = wrap_input(text, 11).concat();
    assert_eq!(joined, text);
}

