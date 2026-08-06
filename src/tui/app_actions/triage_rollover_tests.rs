use super::{logical_day, triage_rollover};

fn dt(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(y, m, d)
        .unwrap()
        .and_hms_opt(h, min, 0)
        .unwrap()
}

fn d(y: i32, m: u32, day: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

#[test]
fn before_rollover_hour_is_previous_day() {
    // 05:59 with a 6 AM rollover still belongs to the previous day.
    assert_eq!(logical_day(dt(2026, 7, 11, 5, 59), 6), d(2026, 7, 10));
}

#[test]
fn at_rollover_hour_is_the_new_day() {
    assert_eq!(logical_day(dt(2026, 7, 11, 6, 0), 6), d(2026, 7, 11));
}

#[test]
fn after_rollover_hour_is_the_same_day() {
    assert_eq!(logical_day(dt(2026, 7, 11, 14, 0), 6), d(2026, 7, 11));
}

#[test]
fn just_past_midnight_is_still_the_previous_day() {
    // The whole point: 00:01 is not a new day under a 6 AM rollover.
    assert_eq!(logical_day(dt(2026, 7, 11, 0, 1), 6), d(2026, 7, 10));
}

#[test]
fn zero_rollover_hour_makes_midnight_the_boundary() {
    assert_eq!(logical_day(dt(2026, 7, 10, 23, 59), 0), d(2026, 7, 10));
    assert_eq!(logical_day(dt(2026, 7, 11, 0, 0), 0), d(2026, 7, 11));
}

#[test]
fn out_of_range_hour_falls_back_to_six() {
    // Hour 30 is nonsense; behave exactly like the 6 AM default.
    assert_eq!(logical_day(dt(2026, 7, 11, 5, 0), 30), d(2026, 7, 10));
    assert_eq!(logical_day(dt(2026, 7, 11, 7, 0), 30), d(2026, 7, 11));
}

#[test]
fn rollover_at_exactly_midnight_does_not_fire() {
    // Session last checked July 10; the clock ticks to 00:00 July 11.
    // With a 6 AM rollover this is still "July 10" — no re-check.
    assert_eq!(
        triage_rollover(d(2026, 7, 10), dt(2026, 7, 11, 0, 0), 6),
        None
    );
}

#[test]
fn working_past_midnight_before_rollover_does_not_fire() {
    assert_eq!(
        triage_rollover(d(2026, 7, 10), dt(2026, 7, 11, 2, 30), 6),
        None
    );
}

#[test]
fn same_day_refresh_does_not_fire() {
    assert_eq!(
        triage_rollover(d(2026, 7, 10), dt(2026, 7, 10, 23, 0), 6),
        None
    );
}

#[test]
fn crossing_the_rollover_fires_with_the_new_day() {
    assert_eq!(
        triage_rollover(d(2026, 7, 10), dt(2026, 7, 11, 7, 0), 6),
        Some(d(2026, 7, 11))
    );
}

#[test]
fn first_refresh_after_rollover_but_past_next_midnight_uses_logical_day() {
    // No refresh happened between the 6 AM rollover and 01:00 the next
    // calendar day. The logical day is still July 11 (calendar July 12),
    // so we adopt July 11 — not the calendar date.
    assert_eq!(
        triage_rollover(d(2026, 7, 10), dt(2026, 7, 12, 1, 0), 6),
        Some(d(2026, 7, 11))
    );
}
