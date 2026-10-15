use std::collections::BTreeSet;

// These fixtures name the break each transform must prevent: stale removed
// edges, lost blockers, duplicate expansion, and global overvalidation.
fn dependency_graph(rows: &[(&str, &[&str])]) -> super::super::dependencies::Graph {
    rows.iter()
        .map(|(id, edges)| {
            (
                id.to_string(),
                edges.iter().map(ToString::to_string).collect(),
            )
        })
        .collect()
}

#[test]
fn dependency_batch_and_diamond_expand_to_unique_surviving_blockers() {
    use super::super::{dependencies::rewrite, model::Dependents};
    let graph = dependency_graph(&[
        ("T1", &[]),
        ("T2", &["T1"]),
        ("T3", &["T1", "T2"]),
        ("T4", &["T2", "T3", "T8"]),
        ("T8", &[]),
    ]);
    let removed = ["T2".into(), "T3".into()].into_iter().collect();
    let result = rewrite(&graph, &removed, Dependents::Rewire).unwrap();
    assert_eq!(
        result,
        dependency_graph(&[("T1", &[]), ("T4", &["T1", "T8"]), ("T8", &[])])
    );
    let result = rewrite(
        &graph,
        &["T1".into(), "T2".into()].into_iter().collect(),
        Dependents::Rewire,
    )
    .unwrap();
    assert_eq!(result["T3"], BTreeSet::default());
    assert_eq!(
        result["T4"],
        ["T3".into(), "T8".into()].into_iter().collect()
    );
}

#[test]
fn dependency_reject_reports_surviving_dependents_and_unlink_preserves_other_edges() {
    use super::super::{dependencies::rewrite, model::Dependents};
    let graph = dependency_graph(&[
        ("T1", &[]),
        ("T2", &["T1"]),
        ("T3", &["T2", "T8"]),
        ("T8", &[]),
    ]);
    let removed = std::iter::once("T2".into()).collect();
    let problem = rewrite(&graph, &removed, Dependents::Reject).unwrap_err();
    assert_eq!(problem.code, "dependents_exist");
    assert_eq!(problem.details["task_id"], "T3");
    assert_eq!(problem.details["prerequisite"], "T2");
    assert_eq!(
        rewrite(&graph, &removed, Dependents::Unlink).unwrap(),
        dependency_graph(&[("T1", &[]), ("T3", &["T8"]), ("T8", &[])])
    );
    assert_eq!(
        rewrite(
            &graph,
            &["T2".into(), "T3".into()].into_iter().collect(),
            Dependents::Reject
        )
        .unwrap(),
        dependency_graph(&[("T1", &[]), ("T8", &[])])
    );
}

fn dependency_problem(rows: &[(&str, &[&str])], removed: &[&str], code: &str, id: &str) {
    use super::super::{dependencies::rewrite, model::Dependents};
    let problem = rewrite(
        &dependency_graph(rows),
        &removed.iter().map(ToString::to_string).collect(),
        Dependents::Rewire,
    )
    .unwrap_err();
    assert_eq!(problem.code, code);
    assert_eq!(problem.details["task_id"], id);
}

#[test]
fn dependency_expansion_cycle_is_rejected() {
    dependency_problem(
        &[("T1", &["T2"]), ("T2", &["T1"]), ("T3", &["T2"])],
        &["T1", "T2"],
        "dependency_cycle",
        "T2",
    );
}

#[test]
fn dependency_new_self_edge_is_rejected() {
    dependency_problem(
        &[("T1", &["T2"]), ("T2", &["T1"])],
        &["T2"],
        "dependency_cycle",
        "T1",
    );
}

#[test]
fn dependency_missing_expanded_leaf_is_rejected() {
    dependency_problem(
        &[("T2", &["T9"]), ("T3", &["T2"])],
        &["T2"],
        "missing_prerequisite",
        "T9",
    );
}

#[test]
fn dependency_missing_removed_target_is_rejected() {
    dependency_problem(&[("T3", &["T2"])], &["T2"], "missing_prerequisite", "T2");
}

#[test]
fn dependency_result_cycle_through_survivors_is_rejected() {
    dependency_problem(
        &[
            ("T1", &["T4"]),
            ("T2", &["T1"]),
            ("T3", &["T2"]),
            ("T4", &["T3"]),
        ],
        &["T2"],
        "dependency_cycle",
        "T3",
    );
}

#[test]
fn dependency_rewrite_preserves_unrelated_invalid_state_and_input() {
    use super::super::{dependencies::rewrite, model::Dependents};
    let graph = dependency_graph(&[
        ("T1", &[]),
        ("T2", &["T1"]),
        ("T3", &["T2"]),
        ("T8", &["T9"]),
        ("T9", &["T8"]),
        ("T10", &["T99"]),
    ]);
    let before = graph.clone();
    assert_eq!(
        rewrite(
            &graph,
            &std::iter::once("T2".into()).collect(),
            Dependents::Rewire
        )
        .unwrap(),
        dependency_graph(&[
            ("T1", &[]),
            ("T3", &["T1"]),
            ("T8", &["T9"]),
            ("T9", &["T8"]),
            ("T10", &["T99"])
        ])
    );
    assert_eq!(
        rewrite(&graph, &BTreeSet::default(), Dependents::Rewire).unwrap(),
        before
    );
    assert_eq!(graph, before);
}

#[test]
fn dependency_unlink_filters_only_removed_ids() {
    use super::super::{dependencies::rewrite, model::Dependents};
    let graph = dependency_graph(&[
        ("T1", &[]),
        ("T2", &["T1"]),
        ("T3", &["T2", "T8"]),
        ("T8", &[]),
    ]);
    assert_eq!(
        rewrite(
            &graph,
            &std::iter::once("T2".into()).collect(),
            Dependents::Unlink
        )
        .unwrap(),
        dependency_graph(&[("T1", &[]), ("T3", &["T8"]), ("T8", &[])])
    );
}

#[test]
fn dependency_csv_tokens_use_normalized_numeric_order() {
    use super::super::dependencies::ordered_prerequisites;
    let ids = ["T10", "T2", "t003", "T1"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(ordered_prerequisites(&ids), ["T1", "T2", "t003", "T10"]);
}

thread_local! {
    static EXPANSION_BUDGET: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

pub(super) fn record_expansion() {
    EXPANSION_BUDGET.with(|budget| {
        if let Some(left) = budget.get() {
            assert!(
                left > 0,
                "shared DAG expansion exceeded linear visit budget"
            );
            budget.set(Some(left - 1));
        }
    });
}

#[test]
fn dependency_layered_dag_expands_once_across_surviving_roots() {
    use super::super::{dependencies::rewrite, model::Dependents};
    let mut graph = dependency_graph(&[("T0", &[]), ("T1", &["T0"])]);
    for index in 2..=45 {
        graph.insert(
            format!("T{index}"),
            [format!("T{}", index - 1), format!("T{}", index - 2)]
                .into_iter()
                .collect(),
        );
    }
    graph.insert("T46".into(), std::iter::once("T45".into()).collect());
    graph.insert("T47".into(), std::iter::once("T44".into()).collect());
    let removed = (1..=45).map(|index| format!("T{index}")).collect();
    EXPANSION_BUDGET.with(|budget| budget.set(Some(100)));
    let result = rewrite(&graph, &removed, Dependents::Rewire).unwrap();
    EXPANSION_BUDGET.with(|budget| budget.set(None));
    assert_eq!(
        result,
        dependency_graph(&[("T0", &[]), ("T46", &["T0"]), ("T47", &["T0"])])
    );
}

#[test]
fn dependency_csv_tokens_keep_malformed_and_overflow_lexical_fallback() {
    let ids = ["T10", "T2", "t003", "T4294967296", "bad", "T?", "T0002"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        super::ordered_prerequisites(&ids),
        ["T0002", "T2", "t003", "T10", "T4294967296", "T?", "bad"]
    );
}
