use crate::tasks::task::Task;
use crate::users::UserId;
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};

/// In-shell fuzzy filter: score `tasks` against `query`, keeping matches in
/// descending score order. An empty query returns every task unchanged.
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
