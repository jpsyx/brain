//! Freshness rules for pulling remote changes before receiver work.

use chrono::{DateTime, TimeDelta, Utc};

pub const MESSAGE_PULL_MAX_AGE: TimeDelta = TimeDelta::hours(2);

#[must_use]
pub fn message_pull_due(last_pull_finished_at: Option<&str>, now: DateTime<Utc>) -> bool {
    let Some(last_pull) =
        last_pull_finished_at.and_then(|value| DateTime::parse_from_rfc3339(value).ok())
    else {
        return true;
    };
    now.signed_duration_since(last_pull.with_timezone(&Utc)) > MESSAGE_PULL_MAX_AGE
}

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, TimeZone, Utc};

    use super::*;

    #[test]
    fn receiver_pull_is_due_without_a_previous_downstream_sync() {
        let now = Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0).unwrap();
        assert!(message_pull_due(None, now));
    }

    #[test]
    fn receiver_pull_is_not_due_until_more_than_two_hours_have_elapsed() {
        let now = Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0).unwrap();
        let exactly_two_hours = (now - TimeDelta::hours(2)).to_rfc3339();
        let just_over_two_hours = (now - TimeDelta::hours(2) - TimeDelta::seconds(1)).to_rfc3339();

        assert!(!message_pull_due(Some(&exactly_two_hours), now));
        assert!(message_pull_due(Some(&just_over_two_hours), now));
    }

    #[test]
    fn malformed_journal_timestamp_is_treated_as_stale() {
        let now = Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0).unwrap();
        assert!(message_pull_due(Some("not-a-time"), now));
    }
}
