use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};

use crate::tasks::task::Task;
use crate::users::UserId;

pub(super) fn filter_tasks<'a>(
    tasks: &'a [Task],
    query: &str,
    assigned_to: Option<&UserId>,
    matcher: &SkimMatcherV2,
) -> Vec<&'a Task> {
    let candidates = tasks
        .iter()
        .filter(|task| assigned_to.is_none_or(|user_id| task.assigned_to == user_id.as_str()));
    if query.trim().is_empty() {
        return candidates.collect();
    }
    let mut scored: Vec<(i64, &Task)> = candidates
        .filter_map(|task| {
            let haystack = format!("{} {}", task.id, task.name);
            matcher
                .fuzzy_match(&haystack, query)
                .map(|score| (score, task))
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, task)| task).collect()
}

#[cfg(test)]
mod tests {
    use fuzzy_matcher::skim::SkimMatcherV2;

    use super::filter_tasks;
    use crate::tasks::task::test_task;
    use crate::users::UserId;

    #[test]
    fn assignment_filter_switches_members_and_can_restore_all() {
        let mut alice = test_task("T1", "not_started");
        alice.assigned_to = "alice".to_owned();
        let mut bob = test_task("T2", "not_started");
        bob.assigned_to = "bob".to_owned();
        let tasks = vec![alice, bob];
        let matcher = SkimMatcherV2::default().ignore_case();
        let alice_id = UserId::parse("alice").expect("valid user");
        let bob_id = UserId::parse("bob").expect("valid user");

        let alice_only = filter_tasks(&tasks, "", Some(&alice_id), &matcher);
        let bob_only = filter_tasks(&tasks, "", Some(&bob_id), &matcher);
        let all = filter_tasks(&tasks, "", None, &matcher);

        assert_eq!(
            alice_only
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec!["T1"]
        );
        assert_eq!(
            bob_only
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec!["T2"]
        );
        assert_eq!(all.len(), 2);
    }
}
