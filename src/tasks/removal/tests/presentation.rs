use super::{fixture::WorkspaceFixture, plan::preview};
use crate::tasks::removal::{
    model::{Dependents, Options},
    presentation,
};

#[test]
fn consent_projection_exposes_named_references_projects_and_handoffs_without_private_bytes() {
    let fixture = WorkspaceFixture::new();
    fixture.write("tasks/tasks.csv", b"task_id,task_name,status,task_type,blocked_by,see_also,last_touched,notes,unknown\nT1,Read spec (1/4),not_started,mit,,,2026-01-01,private-notes,private-column\nT3,Read spec (3/4),not_started,,T1,T1|https://private.invalid/path|private-prose,2026-01-01,,\n");
    fixture.project("projects/alpha");
    let path = fixture
        .workspace
        .root()
        .join("projects/alpha/.METADATA.json");
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    metadata["tasks"] = serde_json::json!(["T1", "T3"]);
    metadata["private_field"] = serde_json::json!("private-metadata");
    std::fs::write(path, serde_json::to_vec(&metadata).unwrap()).unwrap();
    let plan = preview(
        &fixture,
        &["T1"],
        &Options {
            dependents: Dependents::Unlink,
            ..Options::default()
        },
    );
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let projection = presentation::project(&plan);
    let value = serde_json::to_value(&projection).unwrap();
    assert_eq!(value["tasks"][1]["field"], "references");
    assert_eq!(value["tasks"][1]["task"]["task_name"], "Read spec (3/4)");
    assert_eq!(value["tasks"][1]["previous"][0]["task_id"], "T1");
    assert_eq!(value["tasks"][1]["next"], serde_json::json!([]));
    assert_eq!(
        value["projects"][0]["next"][0]["task_name"],
        "Read spec (3/4)"
    );
    assert_eq!(
        value["handoffs"][0]["successor"]["task_name"],
        "Read spec (3/4)"
    );
    let public = format!("{}{}", value, projection.lines().join("\n"));
    for secret in [
        "private-notes",
        "private-column",
        "private.invalid",
        "private-prose",
        "private-metadata",
        "private_field",
    ] {
        assert!(!public.contains(secret), "{public}");
    }
}

#[test]
fn saved_plans_without_display_labels_remain_compatible_but_effect_tampering_fails() {
    let fixture = WorkspaceFixture::new();
    fixture.write("tasks/tasks.csv", b"task_id,task_name,blocked_by,last_touched\nT1,First,H2,old\nT2,Last,T1,old\nT9,Unrelated,,old\n");
    fixture.write(
        "tasks/habits.csv",
        b"task_id,task_name,status\nH2,Walk,not_started\n",
    );
    let options = Options {
        dependents: Dependents::Rewire,
        ..Options::default()
    };
    let plan = preview(&fixture, &["T1"], &options);
    assert_eq!(plan.presentation_labels["H2"].task_name, "Walk");
    assert!(!plan.presentation_labels.contains_key("T9"));
    let mut saved = serde_json::to_value(&plan).unwrap();
    saved.as_object_mut().unwrap().remove("presentation_labels");
    let old: crate::tasks::removal::model::RemovalPlan = serde_json::from_value(saved).unwrap();
    assert!(old.presentation_labels.is_empty());
    assert!(plan.same_execution(&old));
    let mut changed = old.clone();
    changed.edits[0].after.push(b' ');
    assert!(!plan.same_execution(&changed));
    let snapshot = crate::tasks::removal::snapshot::Snapshot::read(
        &fixture.context(),
        super::fixture::today(),
    )
    .unwrap();
    crate::tasks::removal::execute::validate(&snapshot, &old, &old.fingerprint).unwrap();
    assert_eq!(
        crate::tasks::removal::execute::validate(&snapshot, &changed, &changed.fingerprint)
            .unwrap_err()
            .code,
        "stale_plan"
    );
    fixture.write(
        "tasks/habits.csv",
        b"task_id,task_name,status\nH2,Changed name,not_started\n",
    );
    let snapshot = crate::tasks::removal::snapshot::Snapshot::read(
        &fixture.context(),
        super::fixture::today(),
    )
    .unwrap();
    assert_eq!(
        crate::tasks::removal::execute::validate(&snapshot, &old, &old.fingerprint)
            .unwrap_err()
            .code,
        "stale_plan"
    );
}
