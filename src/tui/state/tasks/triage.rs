use regex::RegexBuilder;

use super::TasksState;
use crate::tasks::task::Task;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DailyTriageNudge {
    pub(crate) task_id: String,
    pub(crate) task_label: String,
}

impl TasksState {
    pub(crate) const fn daily_triage_date(&self) -> chrono::NaiveDate {
        self.today
    }

    pub(crate) fn daily_triage_nudge(
        &self,
        enabled: bool,
        disabled: bool,
        pattern: &str,
    ) -> Option<DailyTriageNudge> {
        if !enabled || disabled {
            return None;
        }
        triage_nudge_target(&self.all_habits, pattern, self.today).map(|habit| DailyTriageNudge {
            task_id: habit.id.clone(),
            task_label: habit.name.clone(),
        })
    }
}

fn triage_nudge_target<'a>(
    habits: &'a [Task],
    pattern: &str,
    today: chrono::NaiveDate,
) -> Option<&'a Task> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return None;
    }
    let name_pattern = RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .ok()?;
    let candidates = habits
        .iter()
        .filter(|habit| name_pattern.is_match(&habit.name))
        .collect::<Vec<_>>();
    if candidates.is_empty()
        || candidates
            .iter()
            .any(|habit| habit.is_completed_today(today))
    {
        return None;
    }
    candidates
        .iter()
        .find(|habit| habit.due_date == Some(today))
        .or_else(|| candidates.iter().max_by_key(|habit| habit.due_date))
        .copied()
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::triage_nudge_target;
    use crate::tasks::task::{Task, test_task};

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, day).expect("valid date")
    }

    fn triage(id: &str, due_day: u32, completed_day: Option<u32>) -> Task {
        let mut habit = test_task(id, "not_started");
        habit.name = "Morning Triage (5mins)".to_owned();
        habit.due_date = Some(date(due_day));
        habit.completed_date = completed_day.map(date);
        if completed_day.is_some() {
            habit.status = "done".to_owned();
        }
        habit
    }

    #[test]
    fn current_open_occurrence_is_selected_without_leaking_the_habit() {
        let habits = [
            triage("H31", 23, Some(23)),
            triage("H47", 25, None),
            triage("H41", 24, None),
        ];

        let target = triage_nudge_target(&habits, "morning triage", date(24));

        assert_eq!(target.map(|habit| habit.id.as_str()), Some("H41"));
    }

    #[test]
    fn completion_today_suppresses_every_matching_occurrence() {
        let habits = [
            triage("H31", 23, Some(23)),
            triage("H41", 24, Some(24)),
            triage("H47", 25, None),
        ];

        assert!(triage_nudge_target(&habits, "Morning Triage", date(24)).is_none());
    }

    #[test]
    fn blank_invalid_and_unmatched_patterns_are_silent() {
        let habits = [triage("H41", 24, None)];

        assert!(triage_nudge_target(&habits, "  ", date(24)).is_none());
        assert!(triage_nudge_target(&habits, "Morning [Triage", date(24)).is_none());
        assert!(triage_nudge_target(&habits, "Weekly Review", date(24)).is_none());
    }
}
