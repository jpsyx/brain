use super::{fixture::WorkspaceFixture, plan::preview};
use crate::tasks::removal::model::{Dependents, Options};

#[test]
fn removal_plan_each_guard_preserves_canonical_files_and_withholds_effects() {
    struct Case {
        code: &'static str,
        tasks: &'static str,
        habits: &'static str,
        targets: &'static [&'static str],
        options: Options,
    }
    let ordinary = "task_id,task_name,blocked_by,last_touched\nT17,Send proposal,,old\n";
    let habit = "task_id,task_name,system_key\nH2,Walk,\n";
    let cases = [
        Case {
            code: "target_not_found",
            tasks: ordinary,
            habits: habit,
            targets: &["T99"],
            options: Options::default(),
        },
        Case {
            code: "ambiguous_target",
            tasks: "task_id,task_name\nT17,Send proposal\nT2,Review proposal\n",
            habits: habit,
            targets: &["proposal"],
            options: Options::default(),
        },
        Case {
            code: "mixed_target_kinds",
            tasks: ordinary,
            habits: habit,
            targets: &["T17", "H2"],
            options: Options::default(),
        },
        Case {
            code: "habit_required",
            tasks: ordinary,
            habits: habit,
            targets: &["H2"],
            options: Options::default(),
        },
        Case {
            code: "habit_flag_on_task",
            tasks: ordinary,
            habits: habit,
            targets: &["T17"],
            options: Options {
                habit: true,
                ..Options::default()
            },
        },
        Case {
            code: "managed_task_cannot_delete",
            tasks: ordinary,
            habits: "task_id,task_name,system_key\nH2,Review,brain.triage.daily\n",
            targets: &["H2"],
            options: Options {
                habit: true,
                ..Options::default()
            },
        },
        Case {
            code: "dependents_exist",
            tasks: "task_id,task_name,blocked_by,last_touched\nT17,Send proposal,,old\nT2,Book meeting,T17,old\n",
            habits: habit,
            targets: &["T17"],
            options: Options::default(),
        },
        Case {
            code: "missing_prerequisite",
            tasks: "task_id,task_name,blocked_by,last_touched\nT17,Send proposal,T99,old\nT2,Book meeting,T17,old\n",
            habits: habit,
            targets: &["T17"],
            options: Options {
                dependents: Dependents::Rewire,
                ..Options::default()
            },
        },
        Case {
            code: "dependency_cycle",
            tasks: "task_id,task_name,blocked_by,last_touched\nT17,Send proposal,T2,old\nT2,Book meeting,T17,old\n",
            habits: habit,
            targets: &["T17"],
            options: Options {
                dependents: Dependents::Rewire,
                ..Options::default()
            },
        },
        Case {
            code: "missing_last_touched_column",
            tasks: "task_id,task_name,see_also\nT17,Send proposal,\nT2,Book meeting,T17\n",
            habits: habit,
            targets: &["T17"],
            options: Options::default(),
        },
        Case {
            code: "ambiguous_chunk_family",
            tasks: "task_id,task_name,task_type,status,last_touched\nT17,Read spec (1/4),mit,not_started,old\nT2,Read spec (1/4),,done,old\n",
            habits: habit,
            targets: &["T17", "T2"],
            options: Options::default(),
        },
        Case {
            code: "adapter_unavailable",
            tasks: "task_id,task_name,linear_issue\nT17,Send proposal,EX-7\n",
            habits: habit,
            targets: &["T17"],
            options: Options {
                linked_issue: crate::tasks::removal::model::LinkedIssue::Cancel,
                ..Options::default()
            },
        },
        Case {
            code: "shared_external_link",
            tasks: "task_id,task_name,linear_issue,status\nT17,Send proposal,EX-7,not_started\nT2,Other,EX-7,waiting\n",
            habits: habit,
            targets: &["T17"],
            options: Options::default(),
        },
    ];
    for case in cases {
        let mut fixture = WorkspaceFixture::new();
        if case.code == "shared_external_link" {
            super::external_policy::integration(
                &mut fixture,
                crate::tasks::removal::model::OnRemove::Cancel,
            );
        }
        fixture.write("tasks/tasks.csv", case.tasks.as_bytes());
        fixture.write("tasks/habits.csv", case.habits.as_bytes());
        let plan = preview(&fixture, case.targets, &case.options);
        assert_eq!(plan.blockers[0].code, case.code);
        assert!(plan.edits.is_empty());
        assert!(plan.external.is_empty());
        assert!(plan.successors.is_empty());
        assert_eq!(
            std::fs::read(fixture.workspace.root().join("tasks/tasks.csv")).unwrap(),
            case.tasks.as_bytes()
        );
        assert_eq!(
            std::fs::read(fixture.workspace.root().join("tasks/habits.csv")).unwrap(),
            case.habits.as_bytes()
        );
    }
}
