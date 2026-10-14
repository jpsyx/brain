use super::fixture::{WorkspaceFixture, needles, today};
use crate::tasks::removal::{
    model::{Dependents, Options},
    snapshot::Snapshot,
};

#[test]
fn removal_plan_composes_dependency_reference_and_project_changes_without_writes() {
    let fixture = WorkspaceFixture::new();
    let before = b"task_id,task_name,blocked_by,see_also,last_touched,unknown\nT17,Send proposal,,,,anchor\nT2,Book meeting,T17,T17|https://example.test/T17,2026-01-01,keep\nT3,Unrelated,,,2026-01-01,untouched\n";
    fixture.write("tasks/tasks.csv", before);
    fixture.project("projects/alpha");
    let snapshot = Snapshot::read(&fixture.context(), today()).unwrap();
    let options = Options {
        dependents: Dependents::Unlink,
        ..Options::default()
    };
    let preview = crate::tasks::removal::plan::plan(&snapshot, &needles(&["T17"]), &options);
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    assert_eq!(preview.edits.len(), 2);
    let edit = preview
        .edits
        .iter()
        .find(|edit| edit.relative.to_str() == Some("tasks/tasks.csv"))
        .unwrap();
    assert_eq!(edit.before.as_deref(), Some(before.as_slice()));
    let csv = crate::tasks::complete::parse_csv_bytes(&edit.after).unwrap();
    assert_eq!(csv.rows.len(), 2);
    assert_eq!(csv.rows[0]["blocked_by"], "");
    assert_eq!(csv.rows[0]["see_also"], "https://example.test/T17");
    assert_eq!(csv.rows[0]["last_touched"], "2026-10-14");
    assert_eq!(csv.rows[0]["unknown"], "keep");
    assert_eq!(csv.rows[1]["last_touched"], "2026-01-01");
    assert_eq!(
        std::fs::read(fixture.workspace.root().join("tasks/tasks.csv")).unwrap(),
        before
    );
    let project = preview
        .edits
        .iter()
        .find(|edit| edit.relative.to_str() == Some("projects/alpha/.METADATA.json"))
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&project.after).unwrap()["tasks"],
        serde_json::json!([])
    );
}

pub(super) fn preview(
    fixture: &WorkspaceFixture,
    targets: &[&str],
    options: &Options,
) -> crate::tasks::removal::model::RemovalPlan {
    crate::tasks::removal::plan::plan(
        &Snapshot::read(&fixture.context(), today()).unwrap(),
        &needles(targets),
        options,
    )
}

fn task_after(plan: &crate::tasks::removal::model::RemovalPlan) -> crate::tasks::complete::CsvFile {
    crate::tasks::complete::parse_csv_bytes(
        &plan
            .edits
            .iter()
            .find(|e| e.relative.to_str() == Some("tasks/tasks.csv"))
            .unwrap()
            .after,
    )
    .unwrap()
}

#[test]
fn removal_plan_promotes_eligible_chunk_across_batch_gaps() {
    let fixture = WorkspaceFixture::new();
    fixture.write("tasks/tasks.csv", b"task_id,task_name,task_type,status,blocked_by,start_date,last_touched,assigned_to,completed_date,defer_count\nT1,Read spec (1/4),mit,not_started,,,2026-01-01,member,,7\nT2,Read spec (2/4),,not_started,T1,,2026-01-01,other,,2\nT3,Read spec (3/4),deep|other,in_progress,T2,2026-10-14,2026-01-01,other,2025-01-01,3\nT4,Read spec (4/4),,not_started,T3,,2026-01-01,other,,4\n");
    let options = Options {
        dependents: Dependents::Unlink,
        ..Options::default()
    };
    let plan = preview(&fixture, &["T2", "T1"], &options);
    assert!(plan.blockers.is_empty());
    assert_eq!(plan.successors["T1"], "T3");
    let csv = task_after(&plan);
    assert_eq!(csv.rows[0]["task_type"], "deep|other|mit");
    assert_eq!(csv.rows[0]["assigned_to"], "other");
    assert_eq!(csv.rows[0]["completed_date"], "2025-01-01");
    assert_eq!(csv.rows[0]["defer_count"], "3");
    assert_eq!(csv.rows[1]["last_touched"], "2026-01-01");
}

#[test]
fn removal_plan_chunk_eligibility_does_not_skip_an_ineligible_next_session() {
    for (status, start, blocked, types) in [
        ("waiting", "", "", ""),
        ("backlog", "", "", ""),
        ("not_started", "2026-10-15", "", ""),
        ("not_started", "", "T9", ""),
        ("not_started", "", "", "mit"),
    ] {
        let fixture = WorkspaceFixture::new();
        fixture.write("tasks/tasks.csv", format!("task_id,task_name,task_type,status,blocked_by,start_date,last_touched\nT1,Read spec (1/4),mit,not_started,,,old\nT3,Read spec (3/4),{types},{status},{blocked},{start},old\nT4,Read spec (4/4),,not_started,,,old\nT9,Other,,not_started,,,old\n").as_bytes());
        let plan = preview(&fixture, &["T1"], &Options::default());
        assert!(plan.blockers.is_empty());
        if types == "mit" {
            assert_eq!(plan.successors["T1"], "T3");
        } else {
            assert!(plan.successors.is_empty());
        }
        assert_eq!(task_after(&plan).rows[0]["last_touched"], "old");
        assert_eq!(task_after(&plan).rows[1]["task_type"], "");
    }
}

#[test]
fn removal_plan_rejects_original_chunk_ambiguity_even_when_duplicates_are_deleted() {
    let fixture = WorkspaceFixture::new();
    fixture.write("tasks/tasks.csv", b"task_id,task_name,task_type,status,last_touched\nT1,Read spec (1/4),mit,not_started,old\nT2,Read spec (1/4),,done,old\nT3,Read spec (3/4),,not_started,old\n");
    let plan = preview(&fixture, &["T1", "T2"], &Options::default());
    assert_eq!(plan.blockers[0].code, "ambiguous_chunk_family");
    assert!(plan.edits.is_empty());
}

#[test]
fn removal_plan_missing_touch_column_blocks_only_survivor_edits() {
    let fixture = WorkspaceFixture::new();
    fixture.write(
        "tasks/tasks.csv",
        b"task_id,task_name,see_also\nT17,Send proposal,\nT2,Book meeting,T17\n",
    );
    let plan = preview(&fixture, &["T17"], &Options::default());
    assert_eq!(plan.blockers[0].code, "missing_last_touched_column");
    assert!(plan.edits.is_empty());
    assert!(
        preview(&fixture, &["T17", "T2"], &Options::default())
            .blockers
            .is_empty()
    );
}

#[test]
fn removal_plan_preserves_original_assignment_header_and_raw_survivor_values() {
    for assignment in ["assignee", "assignee,assigned_to"] {
        let fixture = WorkspaceFixture::new();
        let values = if assignment.contains(',') {
            "legacy,current"
        } else {
            "legacy"
        };
        let bytes = format!(
            "task_id,task_name,see_also,last_touched,{assignment},unknown\nT17,Send proposal,,old,{values},keep\nT2,Book meeting,T17,old,{values},untouched\n"
        );
        fixture.write("tasks/tasks.csv", bytes.as_bytes());
        let plan = preview(&fixture, &["T17"], &Options::default());
        let after = &plan.edits[0].after;
        let mut reader = csv::Reader::from_reader(after.as_slice());
        assert_eq!(
            reader.headers().unwrap().iter().collect::<Vec<_>>(),
            bytes.lines().next().unwrap().split(',').collect::<Vec<_>>()
        );
        let record = reader.records().next().unwrap().unwrap();
        assert_eq!(record.get(4), Some("legacy"));
        assert_eq!(record.get(record.len() - 1), Some("untouched"));
    }
}

#[test]
fn removal_plan_canonicalizes_dependency_ids_and_orders_rewired_prerequisites() {
    let fixture = WorkspaceFixture::new();
    fixture.write("tasks/tasks.csv", b"task_id,task_name,blocked_by,last_touched\nT17,Send proposal,t010|T2,old\nT2,First,,old\nT10,Second,,old\nT3,Dependent, t17 |T2,old\nT4,Unrelated, t002 |missing,old\n");
    let plan = preview(
        &fixture,
        &["T17"],
        &Options {
            dependents: Dependents::Rewire,
            ..Options::default()
        },
    );
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let csv = task_after(&plan);
    assert_eq!(csv.rows[2]["blocked_by"], "T2|T10");
    assert_eq!(csv.rows[3]["blocked_by"], " t002 |missing");
}

#[test]
fn removal_plan_explicit_cancel_needs_an_adapter_for_a_nonempty_link() {
    let fixture = WorkspaceFixture::new();
    fixture.write(
        "tasks/tasks.csv",
        b"task_id,task_name,linear_issue\nT17,Send proposal, EX-7 \n",
    );
    let options = Options {
        linked_issue: crate::tasks::removal::model::LinkedIssue::Cancel,
        ..Options::default()
    };
    let plan = preview(&fixture, &["T17"], &options);
    assert_eq!(plan.blockers[0].code, "adapter_unavailable");
    assert!(plan.edits.is_empty());
}

#[test]
fn removal_plan_identity_json_is_strings_and_validates_deserialization() {
    let fixture = WorkspaceFixture::new();
    let plan = preview(&fixture, &["T17"], &Options::default());
    let value = serde_json::to_value(&plan.targets[0]).unwrap();
    assert_eq!(value["csv"], "tasks");
    assert!(value["task_uuid"].is_string());
    assert_eq!(
        serde_json::from_value::<crate::tasks::removal::model::Identity>(value.clone()).unwrap(),
        plan.targets[0]
    );
    let mut invalid = value;
    invalid["task_uuid"] = serde_json::json!("invalid");
    assert!(serde_json::from_value::<crate::tasks::removal::model::Identity>(invalid).is_err());
}

#[test]
fn removal_plan_repeats_deterministically_for_duplicate_and_reordered_targets() {
    let fixture = WorkspaceFixture::new();
    fixture.write(
        "tasks/tasks.csv",
        b"task_id,task_name\nT17,Send proposal\nT2,Book meeting\nT3,Other\n",
    );
    let a = preview(&fixture, &["T17", "T2"], &Options::default());
    for targets in [&["T2", "t017", "T17"][..], &["T17", "T2"][..]] {
        let b = preview(&fixture, targets, &Options::default());
        assert_eq!(
            serde_json::to_vec(&a).unwrap(),
            serde_json::to_vec(&b).unwrap()
        );
        assert_eq!(a.edits, b.edits);
    }
}

#[test]
fn removal_plan_keeps_existing_raw_blocker_tokens_while_serializing_changed_fields() {
    for (policy, expected) in [
        (Dependents::Unlink, " t002 | T010 "),
        (Dependents::Rewire, " t002 | T010 |T20"),
    ] {
        let fixture = WorkspaceFixture::new();
        fixture.write("tasks/tasks.csv", b"task_id,task_name,blocked_by,last_touched\nT17,Send proposal,t020,old\nT2,First,,old\nT10,Second,,old\nT20,Third,,old\nT3,Dependent,T17| t002 |T2| T010 ,old\n");
        let plan = preview(
            &fixture,
            &["T17"],
            &Options {
                dependents: policy,
                ..Options::default()
            },
        );
        assert!(plan.blockers.is_empty());
        assert_eq!(task_after(&plan).rows[3]["blocked_by"], expected);
    }
}

#[test]
fn removal_plan_unfinished_habit_prevents_mit_transfer() {
    let fixture = WorkspaceFixture::new();
    fixture.write("tasks/tasks.csv", b"task_id,task_name,task_type,status,blocked_by,last_touched\nT17,Read spec (1/4),mit,not_started,,old\nT3,Read spec (3/4),,not_started,H2,old\n");
    fixture.write(
        "tasks/habits.csv",
        b"task_id,task_name,status\nH2,Walk,not_started\n",
    );
    let plan = preview(&fixture, &["T17"], &Options::default());
    assert!(plan.blockers.is_empty());
    assert!(plan.successors.is_empty());
    assert_eq!(task_after(&plan).rows[0]["task_type"], "");
    assert_eq!(task_after(&plan).rows[0]["last_touched"], "old");
}

#[test]
fn removal_plan_future_due_date_and_done_blocker_do_not_prevent_mit_transfer() {
    let fixture = WorkspaceFixture::new();
    fixture.write("tasks/tasks.csv", b"task_id,task_name,task_type,status,blocked_by,start_date,due_date,last_touched\nT17,Read spec (1/4),mit,not_started,,,,old\nT2,Read spec (2/4),,done,,,,old\nT3,Read spec (3/4),,not_started,T9,,2027-01-01,old\nT9,Other,,done,,,,old\n");
    let plan = preview(&fixture, &["T17"], &Options::default());
    assert!(plan.blockers.is_empty());
    assert_eq!(plan.successors["T17"], "T3");
    assert_eq!(task_after(&plan).rows[1]["task_type"], "mit");
}

#[test]
fn removal_plan_agenda_handoffs_include_non_mit_existing_mit_and_multiple_anchors() {
    for (anchor_types, successor_types, expected_types, touched) in [
        ("", "", "", "old"),
        ("mit", "mit|deep", "mit|deep", "old"),
        ("mit", "deep", "deep|mit", "2026-10-14"),
    ] {
        let fixture = WorkspaceFixture::new();
        fixture.write("tasks/tasks.csv", format!("task_id,task_name,task_type,status,last_touched\nT1,Read spec (1/4),{anchor_types},not_started,old\nT2,Read spec (2/4),{anchor_types},not_started,old\nT3,Read spec (3/4),{successor_types},not_started,old\n").as_bytes());
        let plan = preview(&fixture, &["T2", "T1"], &Options::default());
        assert!(plan.blockers.is_empty());
        assert_eq!(
            plan.successors,
            std::collections::BTreeMap::from([
                ("T1".into(), "T3".into()),
                ("T2".into(), "T3".into())
            ])
        );
        let csv = task_after(&plan);
        assert_eq!(csv.rows[0]["task_type"], expected_types);
        assert_eq!(csv.rows[0]["last_touched"], touched);
    }
}

#[test]
fn removal_plan_visibility_uses_existing_datetime_and_malformed_date_semantics() {
    for (start, handoff) in [
        ("2026-10-14T23:59", true),
        ("2026-10-15T00:00", false),
        ("invalid", true),
    ] {
        let fixture = WorkspaceFixture::new();
        fixture.write("tasks/tasks.csv", format!("task_id,task_name,task_type,status,start_date,last_touched\nT17,Read spec (1/4),mit,not_started,,old\nT3,Read spec (3/4),,not_started,{start},old\n").as_bytes());
        let plan = preview(&fixture, &["T17"], &Options::default());
        assert!(plan.blockers.is_empty());
        assert_eq!(
            plan.successors.get("T17").map(String::as_str),
            handoff.then_some("T3")
        );
    }
}
