//! The backlog: listing it, and the two maintenance passes that keep it from
//! becoming a graveyard.
//!
//! Both passes are **deliberately silent** in normal use. They run inside
//! triage, and a user who parked something six months ago does not want a
//! report about forgetting it — that is what parking meant. `--dry-run` and
//! `--report` exist for a human checking the rule, not for the routine pass.

pub(crate) mod dedupe;
pub(crate) mod list;
pub(crate) mod purge;

#[cfg(test)]
mod tests;

use chrono::NaiveDate;

/// Six calendar months before `date`, clamping the day for short months
/// (31 Aug − 6 months is 28/29 Feb, not an invalid 31 Feb).
pub(crate) fn minus_six_months(date: NaiveDate) -> Option<NaiveDate> {
    use chrono::Datelike;
    let mut month = i32::try_from(date.month()).ok()? - 6;
    let mut year = date.year();
    while month <= 0 {
        month += 12;
        year -= 1;
    }
    let month = u32::try_from(month).ok()?;
    (1..=date.day())
        .rev()
        .find_map(|day| NaiveDate::from_ymd_opt(year, month, day))
}
