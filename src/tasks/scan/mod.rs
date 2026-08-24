//! Read-only scans over `tasks.csv`: the calendar arithmetic triage runs on.
//!
//! These exist as code, not as prose in a skill, for one reason: an LLM is bad
//! at date maths and worse at doing it the same way twice. Each scan is a pure
//! function of the rows and today's date, so the same CSV always yields the
//! same list.

pub(crate) mod chronic;
pub(crate) mod linked;
pub(crate) mod waiting;

#[cfg(test)]
mod tests;
