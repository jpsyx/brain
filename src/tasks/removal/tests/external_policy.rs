use super::fixture::WorkspaceFixture;
use super::plan::preview;
use crate::tasks::removal::model::Options;

pub(super) fn integration(
    fixture: &mut WorkspaceFixture,
    on_remove: crate::tasks::removal::model::OnRemove,
) {
    use crate::tasks::removal::model::{AdapterSnapshot, IntegrationPolicy};
    fixture.integration.policy = Some(IntegrationPolicy {
        adapter: "tracker".into(),
        link_field: "linear_issue".into(),
        on_remove,
    });
    fixture.integration.adapter = Some(AdapterSnapshot {
        argv: vec!["fake-adapter".into()],
        environment_names: std::collections::BTreeSet::from(["TEST_TOKEN".into()]),
        timeout_ms: 50,
    });
}

#[test]
fn removal_plan_external_policy_precedence_and_deduplicated_stable_actions() {
    use crate::tasks::removal::model::{LinkedIssue, OnRemove};
    for (configured, option, expected) in [
        (None, LinkedIssue::Configured, 0),
        (None, LinkedIssue::Keep, 0),
        (Some(OnRemove::Keep), LinkedIssue::Configured, 0),
        (Some(OnRemove::Cancel), LinkedIssue::Configured, 2),
        (Some(OnRemove::Cancel), LinkedIssue::Keep, 0),
        (Some(OnRemove::Keep), LinkedIssue::Cancel, 2),
    ] {
        let mut fixture = WorkspaceFixture::new();
        fixture.write("tasks/tasks.csv", b"task_id,task_name,linear_issue,status\nT17,Send proposal, EX-7 ,not_started\nT2,Book meeting,EX-7,in_progress\nT3,Other,EX-2,not_started\n");
        if let Some(policy) = configured {
            integration(&mut fixture, policy);
        }
        let options = Options {
            linked_issue: option,
            ..Options::default()
        };
        let plan = preview(&fixture, &["T17", "T3", "T2", "T17"], &options);
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert_eq!(plan.external.len(), expected);
        if expected != 0 {
            assert_eq!(
                plan.external
                    .iter()
                    .map(|action| action.issue.as_str())
                    .collect::<Vec<_>>(),
                ["EX-2", "EX-7"]
            );
            assert!(
                plan.external
                    .iter()
                    .all(|action| action.adapter == "tracker" && action.action_id.len() == 64)
            );
            assert_ne!(plan.external[0].action_id, plan.external[1].action_id);
            assert_eq!(
                serde_json::to_vec(&plan).unwrap(),
                serde_json::to_vec(&preview(&fixture, &["T2", "T3", "T17"], &options)).unwrap()
            );
        }
    }
}

#[test]
fn removal_plan_external_sharing_checks_both_csvs_and_all_unfinished_states() {
    use crate::tasks::removal::model::{LinkedIssue, OnRemove};
    for csv in ["tasks/tasks.csv", "tasks/habits.csv"] {
        for status in [
            "not_started",
            "in_progress",
            "waiting",
            "backlog",
            "",
            "done",
        ] {
            let mut fixture = WorkspaceFixture::new();
            integration(&mut fixture, OnRemove::Cancel);
            fixture.write(
                "tasks/tasks.csv",
                b"task_id,task_name,linear_issue,status\nT17,Send proposal,EX-7,not_started\n",
            );
            let prefix = if csv == "tasks/tasks.csv" { "T" } else { "H" };
            let header = "task_id,task_name,linear_issue,status\n";
            let anchor = if prefix == "T" {
                "T17,Send proposal,EX-7,not_started\n"
            } else {
                ""
            };
            fixture.write(
                csv,
                format!("{header}{anchor}{prefix}2,Other, EX-7 ,{status}\n").as_bytes(),
            );
            let plan = preview(&fixture, &["T17"], &Options::default());
            if status == "done" {
                assert!(plan.blockers.is_empty());
                assert_eq!(plan.external.len(), 1);
            } else {
                assert_eq!(plan.blockers[0].code, "shared_external_link");
                assert!(plan.edits.is_empty());
                assert!(plan.external.is_empty());
            }
            let keep = preview(
                &fixture,
                &["T17"],
                &Options {
                    linked_issue: LinkedIssue::Keep,
                    ..Options::default()
                },
            );
            assert!(keep.blockers.is_empty());
        }
    }
}

#[test]
fn removal_plan_cancel_is_a_noop_without_links_and_blocks_unavailable_adapter() {
    use crate::tasks::removal::model::{LinkedIssue, OnRemove};
    let mut fixture = WorkspaceFixture::new();
    let cancel = Options {
        linked_issue: LinkedIssue::Cancel,
        ..Options::default()
    };
    assert!(preview(&fixture, &["T17"], &cancel).blockers.is_empty());
    fixture.write(
        "tasks/tasks.csv",
        b"task_id,task_name,linear_issue\nT17,Send proposal,EX-7\n",
    );
    integration(&mut fixture, OnRemove::Cancel);
    fixture.integration.adapter = None;
    let plan = preview(&fixture, &["T17"], &Options::default());
    assert_eq!(plan.blockers[0].code, "adapter_unavailable");
    assert!(plan.edits.is_empty());
    assert!(plan.external.is_empty());
}
