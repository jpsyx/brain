//! Parsing and evaluation of the positional date selector argument.

use anyhow::{Result, bail};
use chrono::{Datelike, Duration, NaiveDate, Weekday};

use crate::tasks::task::Task;

#[derive(Debug, Clone)]
pub enum Selector {
    /// No date filter.
    All,
    /// Today's agenda: due today OR past-due (not done).
    Today,
    /// Tasks whose due_date equals this date.
    OnDate(NaiveDate),
    /// Tasks whose due_date falls in the Mon..Sun week containing this Monday.
    Week(NaiveDate),
}

pub fn parse_selector(raw: Option<&str>, today: NaiveDate) -> Result<Selector> {
    let Some(s) = raw else {
        return Ok(Selector::Today);
    };
    let lower = s.trim().to_ascii_lowercase();

    if lower.is_empty() {
        return Ok(Selector::Today);
    }
    match lower.as_str() {
        "all" => return Ok(Selector::All),
        "today" => return Ok(Selector::Today),
        "tomorrow" => return Ok(Selector::OnDate(today + Duration::days(1))),
        "yesterday" => return Ok(Selector::OnDate(today - Duration::days(1))),
        "week" | "this-week" | "this_week" => {
            let dow = i64::from(today.weekday().num_days_from_monday());
            return Ok(Selector::Week(today - Duration::days(dow)));
        }
        _ => {}
    }

    let (next_only, day_str) = lower
        .strip_prefix("next ")
        .map_or((false, lower.as_str()), |rest| (true, rest.trim()));

    if let Some(weekday) = parse_weekday(day_str) {
        let today_idx = i64::from(today.weekday().num_days_from_monday());
        let target_idx = i64::from(weekday.num_days_from_monday());
        let mut diff = target_idx - today_idx;
        if diff < 0 {
            diff += 7;
        }
        if next_only && diff == 0 {
            diff = 7;
        }
        return Ok(Selector::OnDate(today + Duration::days(diff)));
    }

    if let Ok(date) = NaiveDate::parse_from_str(&lower, "%Y-%m-%d") {
        return Ok(Selector::OnDate(date));
    }

    bail!(
        "could not parse '{s}' as a selector. Try 'all', 'today', 'tomorrow', \
         a weekday name, or YYYY-MM-DD."
    )
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    match s {
        "monday" | "mon" => Some(Weekday::Mon),
        "tuesday" | "tue" | "tues" => Some(Weekday::Tue),
        "wednesday" | "wed" => Some(Weekday::Wed),
        "thursday" | "thu" | "thur" | "thurs" => Some(Weekday::Thu),
        "friday" | "fri" => Some(Weekday::Fri),
        "saturday" | "sat" => Some(Weekday::Sat),
        "sunday" | "sun" => Some(Weekday::Sun),
        _ => None,
    }
}

#[must_use]
pub fn matches(sel: &Selector, t: &Task, today: NaiveDate) -> bool {
    match sel {
        Selector::All => true,
        Selector::Today => t
            .due_date
            .is_some_and(|d| d == today || (d < today && !t.is_done())),
        Selector::OnDate(d) => t.due_date == Some(*d),
        Selector::Week(monday) => t
            .due_date
            .is_some_and(|dd| dd >= *monday && dd <= *monday + Duration::days(6)),
    }
}

#[must_use]
pub fn titles(sel: &Selector, today: NaiveDate) -> (String, String) {
    match sel {
        Selector::All => ("All tasks".to_owned(), String::new()),
        Selector::Today => ("Today".to_owned(), format!("{today} · includes past-due")),
        Selector::OnDate(d) => {
            let label = if *d == today {
                "Today".to_owned()
            } else if *d == today + Duration::days(1) {
                "Tomorrow".to_owned()
            } else if *d == today - Duration::days(1) {
                "Yesterday".to_owned()
            } else {
                d.format("%A, %b %-d, %Y").to_string()
            };
            (label, d.to_string())
        }
        Selector::Week(monday) => {
            let end = *monday + Duration::days(6);
            ("This week".to_owned(), format!("{monday} to {end}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Selector, parse_selector};
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn none_or_empty_means_today() {
        let today = d(2026, 6, 23);
        assert!(matches!(
            parse_selector(None, today).unwrap(),
            Selector::Today
        ));
        assert!(matches!(
            parse_selector(Some(""), today).unwrap(),
            Selector::Today
        ));
    }

    #[test]
    fn tomorrow_and_yesterday_shift_by_one_day() {
        let today = d(2026, 6, 23);
        let Selector::OnDate(tomorrow) = parse_selector(Some("tomorrow"), today).unwrap() else {
            panic!("expected OnDate")
        };
        assert_eq!(tomorrow, d(2026, 6, 24));
        let Selector::OnDate(yesterday) = parse_selector(Some("yesterday"), today).unwrap() else {
            panic!("expected OnDate")
        };
        assert_eq!(yesterday, d(2026, 6, 22));
    }

    #[test]
    fn week_resolves_to_monday_of_current_week() {
        // 2026-06-23 is a Tuesday → Monday is 2026-06-22.
        let today = d(2026, 6, 23);
        let Selector::Week(monday) = parse_selector(Some("week"), today).unwrap() else {
            panic!("expected Week")
        };
        assert_eq!(monday, d(2026, 6, 22));
    }

    #[test]
    fn iso_date_token_parses() {
        let today = d(2026, 6, 23);
        let Selector::OnDate(date) = parse_selector(Some("2026-12-31"), today).unwrap() else {
            panic!("expected OnDate")
        };
        assert_eq!(date, d(2026, 12, 31));
    }

    #[test]
    fn weekday_lands_on_the_nearest_future_occurrence_excluding_today() {
        // Today is Tuesday (idx 1). Friday (idx 4) is +3 days.
        let today = d(2026, 6, 23);
        let Selector::OnDate(friday) = parse_selector(Some("friday"), today).unwrap() else {
            panic!("expected OnDate")
        };
        assert_eq!(friday, d(2026, 6, 26));
        // Same weekday returns today (diff=0); short alias works too.
        let Selector::OnDate(tue) = parse_selector(Some("tue"), today).unwrap() else {
            panic!("expected OnDate")
        };
        assert_eq!(tue, today);
    }

    #[test]
    fn next_weekday_skips_today_when_same_weekday() {
        // Today is Tuesday — `next tuesday` lands on +7, not today.
        let today = d(2026, 6, 23);
        let Selector::OnDate(next_tue) = parse_selector(Some("next tuesday"), today).unwrap()
        else {
            panic!("expected OnDate")
        };
        assert_eq!(next_tue, d(2026, 6, 30));
    }

    #[test]
    fn garbage_token_errors() {
        let today = d(2026, 6, 23);
        assert!(parse_selector(Some("definitely-not-a-date"), today).is_err());
    }
}
