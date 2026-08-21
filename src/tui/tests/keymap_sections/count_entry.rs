#[test]
fn first_digit_starts_a_count() {
    assert_eq!(accumulate_count(None, 3), Some(3));
}

#[test]
fn subsequent_digits_shift_and_append() {
    // Typing `1` then `2` then `0` builds 120 (e.g. 120j).
    let c = accumulate_count(None, 1);
    let c = accumulate_count(c, 2);
    assert_eq!(accumulate_count(c, 0), Some(120));
}

#[test]
fn leading_zero_is_not_a_count() {
    // A bare `0` doesn't start a count, leaving the key free.
    assert_eq!(accumulate_count(None, 0), None);
}

#[test]
fn zero_extends_an_in_progress_count() {
    // But `0` after a non-zero digit is a normal digit: `1` then `0`.
    assert_eq!(accumulate_count(Some(1), 0), Some(10));
}
