//! Pure dependency repair for a batch of removed task rows.

use std::collections::{BTreeMap, BTreeSet};

use super::model::{Dependents, Problem};

pub(crate) type Graph = BTreeMap<String, BTreeSet<String>>;

/// Remove rows and repair surviving incoming edges without changing the input.
///
/// # Errors
/// Returns `dependents_exist` for rejected incoming edges, or
/// `missing_prerequisite` / `dependency_cycle` on traversed invalid paths.
pub(crate) fn rewrite(
    graph: &Graph,
    removed: &BTreeSet<String>,
    policy: Dependents,
) -> Result<Graph, Problem> {
    let mut result = Graph::new();
    let mut changed = BTreeSet::new();
    for (id, prerequisites) in graph {
        if removed.contains(id) {
            continue;
        }
        let mut next = BTreeSet::new();
        for prerequisite in prerequisites {
            if !removed.contains(prerequisite) {
                next.insert(prerequisite.clone());
                continue;
            }
            changed.insert(id.clone());
            match policy {
                Dependents::Reject => {
                    return Err(Problem::new(
                        "dependents_exist",
                        "surviving task depends on a removed task",
                        serde_json::json!({"task_id": id, "prerequisite": prerequisite}),
                    ));
                }
                Dependents::Unlink => {}
                Dependents::Rewire => {
                    next.extend(expand(prerequisite, graph, removed, &mut BTreeSet::new())?);
                }
            }
        }
        result.insert(id.clone(), next);
    }
    // Existing disconnected corruption must not block an unrelated deletion.
    let mut validated = BTreeSet::new();
    for id in changed {
        validate(&id, &result, &mut BTreeSet::new(), &mut validated)?;
    }
    Ok(result)
}

fn expand(
    id: &str,
    graph: &Graph,
    removed: &BTreeSet<String>,
    visiting: &mut BTreeSet<String>,
) -> Result<BTreeSet<String>, Problem> {
    let prerequisites = graph.get(id).ok_or_else(|| missing(id))?;
    if !removed.contains(id) {
        return Ok(BTreeSet::from([id.to_owned()]));
    }
    if !visiting.insert(id.to_owned()) {
        return Err(cycle(id));
    }
    let mut result = BTreeSet::new();
    for prerequisite in prerequisites {
        result.extend(expand(prerequisite, graph, removed, visiting)?);
    }
    visiting.remove(id);
    Ok(result)
}

fn validate(
    id: &str,
    graph: &Graph,
    visiting: &mut BTreeSet<String>,
    validated: &mut BTreeSet<String>,
) -> Result<(), Problem> {
    if validated.contains(id) {
        return Ok(());
    }
    let prerequisites = graph.get(id).ok_or_else(|| missing(id))?;
    if !visiting.insert(id.to_owned()) {
        return Err(cycle(id));
    }
    for prerequisite in prerequisites {
        validate(prerequisite, graph, visiting, validated)?;
    }
    visiting.remove(id);
    validated.insert(id.to_owned());
    Ok(())
}

fn missing(id: &str) -> Problem {
    Problem::new(
        "missing_prerequisite",
        "traversed prerequisite has no task row",
        serde_json::json!({"task_id": id}),
    )
}

fn cycle(id: &str) -> Problem {
    Problem::new(
        "dependency_cycle",
        "traversed dependency path contains a cycle",
        serde_json::json!({"task_id": id}),
    )
}

/// Prerequisite tokens ordered by normalized numeric ID for CSV serialization.
/// Keeps original tokens; malformed IDs sort lexically after valid IDs.
pub(crate) fn ordered_prerequisites(ids: &BTreeSet<String>) -> Vec<&str> {
    let mut tokens: Vec<_> = ids.iter().map(String::as_str).collect();
    tokens.sort_by_cached_key(|id| {
        let number = crate::tasks::complete::normalize_id(id)
            .ok()
            .and_then(|normalized| normalized[1..].parse::<u32>().ok());
        (
            number.is_none(),
            number.unwrap_or_default(),
            (*id).to_owned(),
        )
    });
    tokens
}

#[cfg(test)]
#[path = "tests/dependencies.rs"]
mod tests;

pub(super) fn substitute(
    graph: &Graph,
    replacements: &BTreeMap<String, String>,
) -> Result<Graph, Problem> {
    let mut result = Graph::new();
    let mut changed = BTreeSet::new();
    for (id, prerequisites) in graph {
        if replacements.contains_key(id) {
            continue;
        }
        let next: BTreeSet<_> = prerequisites
            .iter()
            .map(|item| replacements.get(item).unwrap_or(item).clone())
            .collect();
        if next != *prerequisites {
            changed.insert(id.clone());
        }
        result.insert(id.clone(), next);
    }
    let mut validated = BTreeSet::new();
    for id in changed {
        validate(&id, &result, &mut BTreeSet::new(), &mut validated)?;
    }
    Ok(result)
}
