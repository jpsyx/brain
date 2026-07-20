//! Task ordering for a materialized view: the `--sort` strategies and the
//! priority-rank key they share.

use chrono::NaiveDate;

use crate::tasks::task::Task;

#[must_use]
pub fn priority_rank(p: &str) -> u8 {
    match p {
        "p0" => 0,
        "p1" => 1,
        "p2" => 2,
        "p3" => 3,
        "p4" => 4,
        _ => 9,
    }
}

pub fn sort_tasks(tasks: &mut [Task], by: &str) {
    match by.to_ascii_lowercase().as_str() {
        "due" => tasks.sort_by(|a, b| {
            (
                a.due_date.unwrap_or(NaiveDate::MAX),
                priority_rank(&a.priority),
                &a.id,
            )
                .cmp(&(
                    b.due_date.unwrap_or(NaiveDate::MAX),
                    priority_rank(&b.priority),
                    &b.id,
                ))
        }),
        "created" | "touched" => tasks.sort_by(|a, b| {
            (b.last_touched.unwrap_or(NaiveDate::MIN), &b.id)
                .cmp(&(a.last_touched.unwrap_or(NaiveDate::MIN), &a.id))
        }),
        "defer" => tasks.sort_by(|a, b| {
            (std::cmp::Reverse(a.defer_count), priority_rank(&a.priority))
                .cmp(&(std::cmp::Reverse(b.defer_count), priority_rank(&b.priority)))
        }),
        // priority (default)
        _ => tasks.sort_by(|a, b| {
            (
                priority_rank(&a.priority),
                a.due_date.unwrap_or(NaiveDate::MAX),
                &a.id,
            )
                .cmp(&(
                    priority_rank(&b.priority),
                    b.due_date.unwrap_or(NaiveDate::MAX),
                    &b.id,
                ))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::priority_rank;

    #[test]
    fn priority_rank_orders_p0_first_and_unknown_last() {
        assert!(priority_rank("p0") < priority_rank("p1"));
        assert!(priority_rank("p4") < priority_rank("p999"));
    }
}
