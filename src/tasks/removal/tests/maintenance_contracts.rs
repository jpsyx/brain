use crate::tasks::removal::maintenance::{DuplicateReplacement, MaintenanceIntent};
#[derive(serde::Serialize)]
struct Legacy<'a> {
    schema_version: u32,
    workspace_uuid: &'a str,
    actor_id: &'a str,
    execution_date: &'a str,
    options: &'a Options,
    targets: &'a std::collections::BTreeSet<crate::tasks::removal::fingerprint::TargetKey>,
    tasks: &'a Option<Vec<u8>>,
    habits: &'a Option<Vec<u8>>,
    projects: &'a std::collections::BTreeMap<&'a str, &'a [u8]>,
    integration: &'a crate::tasks::removal::model::IntegrationSnapshot,
    managed_habits_enabled: bool,
}

use super::fixture::{WorkspaceFixture, needles};
use crate::tasks::complete::{field, read_csv};
use crate::tasks::removal::{
    apply,
    maintenance::{MaintenanceKind, preview_maintenance},
    model::Options,
    preview, resume,
    snapshot::Snapshot,
};
use crate::tasks::store_transaction::{JournalState, TransactionStep};
fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}
fn fixture() -> WorkspaceFixture {
    let f = WorkspaceFixture::new();
    f.write("tasks/tasks.csv", b"task_id,task_name,status,backlogged_date,project,blocked_by,last_touched\nT1,Old,backlog,2000-01-01,atlas,,\nT2,Active,not_started,,atlas,T1,\n");
    f
}
#[test]
fn backlog_removal_cleanup_archive_resolution_is_scoped_unambiguous_and_active_first() {
    let f = fixture();
    f.project("archive/other/atlas");
    let ignored = preview_maintenance(&f.command(), MaintenanceKind::Purge, today())
        .unwrap()
        .plan
        .unwrap();
    assert!(
        ignored
            .edits
            .iter()
            .all(|edit| !edit.relative.ends_with("notes.md"))
    );
    assert!(
        ignored
            .warnings
            .iter()
            .any(|warning| warning.code == "missing_project")
    );
    f.project("archive/projects/old/atlas");
    let archived = preview_maintenance(&f.command(), MaintenanceKind::Purge, today())
        .unwrap()
        .plan
        .unwrap();
    assert!(
        archived
            .edits
            .iter()
            .any(|edit| edit.relative.to_str() == Some("archive/projects/old/atlas/notes.md"))
    );
    f.project("archive/projects/older/atlas");
    assert_eq!(
        preview_maintenance(&f.command(), MaintenanceKind::Purge, today())
            .err()
            .unwrap()
            .code,
        "ambiguous_project"
    );
    f.project("projects/atlas");
    let active = preview_maintenance(&f.command(), MaintenanceKind::Purge, today())
        .unwrap()
        .plan
        .unwrap();
    assert!(
        active
            .edits
            .iter()
            .any(|edit| edit.relative.to_str() == Some("projects/atlas/notes.md"))
    );
    assert_eq!(
        active
            .edits
            .iter()
            .filter(|edit| edit.relative.ends_with("notes.md"))
            .count(),
        1
    );
}
#[test]
fn backlog_removal_cleanup_invalid_history_or_notes_never_changes_canonical_files() {
    for kind in ["history", "utf8", "symlink"] {
        let f = fixture();
        f.project("projects/atlas");
        match kind {
            "history" => {
                let path = f.workspace.root().join("projects/atlas/.METADATA.json");
                let mut value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                value["deleted_backlog_tasks"] = "invalid".into();
                std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            "utf8" => f.write("projects/atlas/notes.md", &[0xff]),
            _ => {
                f.write("elsewhere.md", b"outside notes");
                std::os::unix::fs::symlink(
                    f.workspace.root().join("elsewhere.md"),
                    f.workspace.root().join("projects/atlas/notes.md"),
                )
                .unwrap();
            }
        }
        let before = std::fs::read(f.workspace.root().join("tasks/tasks.csv")).unwrap();
        match preview_maintenance(&f.command(), MaintenanceKind::Purge, today()) {
            Ok(result) => {
                let plan = result.plan.unwrap();
                assert!(!plan.blockers.is_empty());
                assert!(apply(&f.command(), &plan, &plan.fingerprint, today()).is_err());
            }
            Err(error) => assert!(matches!(
                error.code.as_str(),
                "invalid_project_notes" | "invalid_workspace_path"
            )),
        }
        assert_eq!(
            std::fs::read(f.workspace.root().join("tasks/tasks.csv")).unwrap(),
            before
        );
    }
}
#[test]
fn backlog_removal_cleanup_unrelated_notes_do_not_stale_and_missing_notes_recover_once() {
    let f = fixture();
    f.project("projects/atlas");
    f.project("projects/other");
    let context = f.command();
    let plan = preview_maintenance(&context, MaintenanceKind::Purge, today())
        .unwrap()
        .plan
        .unwrap();
    assert!(
        plan.edits
            .iter()
            .find(|edit| edit.relative.to_str() == Some("projects/atlas/notes.md"))
            .unwrap()
            .before
            .is_none()
    );
    f.write("projects/other/notes.md", b"unrelated change");
    let mut stopped = false;
    let error = crate::tasks::removal::execute::apply_with_hook(
        &context,
        &plan,
        &plan.fingerprint,
        today(),
        |step| {
            if step == TransactionStep::PublishJournalWrite(JournalState::Committed) {
                stopped = true;
            }
            if stopped {
                Err(std::io::Error::other("simulated crash"))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    let id = error.details["operation_id"].as_str().unwrap();
    let receipt = resume(&context, id).unwrap();
    assert!(receipt.committed);
    let notes =
        std::fs::read_to_string(f.workspace.root().join("projects/atlas/notes.md")).unwrap();
    assert_eq!(notes.matches("**T1**").count(), 1);
    assert_eq!(resume(&context, id).unwrap(), receipt);
}
#[test]
fn backlog_removal_cleanup_dedupe_changes_exact_reverse_links_and_retains_forward_fields() {
    let f = WorkspaceFixture::new();
    f.write("tasks/tasks.csv", b"task_id,task_name,status,backlogged_date,created_date,project,blocked_by,see_also,last_touched,linear_issue\nT1,Write plan,backlog,2000-01-01,,atlas,,,,EX-1\nT2,Write plan,not_started,,2020-02-01,other,,,,EX-2\nT3,Review,not_started,,,,T1,T1|T2|x|x|T10,,\n");
    f.write(
        "tasks/habits.csv",
        b"task_id,task_name,blocked_by,see_also,last_touched\nH1,Check,T1,T1|T2,\n",
    );
    f.project("projects/atlas");
    let path = f.workspace.root().join("projects/atlas/.METADATA.json");
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    metadata["tasks"] = serde_json::json!(["T1", "T2", " T1 ", "other", "other"]);
    std::fs::write(&path, serde_json::to_vec(&metadata).unwrap()).unwrap();
    let plan = preview_maintenance(&f.command(), MaintenanceKind::Dedupe, today())
        .unwrap()
        .plan
        .unwrap();
    assert!(plan.external.is_empty());
    assert!(plan.successors.is_empty());
    apply(&f.command(), &plan, &plan.fingerprint, today()).unwrap();
    let rows = read_csv(&f.workspace.root().join("tasks/tasks.csv"))
        .unwrap()
        .rows;
    assert_eq!(field(&rows[0], "project"), "other");
    assert_eq!(field(&rows[0], "linear_issue"), "EX-2");
    assert_eq!(field(&rows[1], "see_also"), "T2|x|x|T10");
    assert_eq!(
        field(
            &read_csv(&f.workspace.root().join("tasks/habits.csv"))
                .unwrap()
                .rows[0],
            "blocked_by"
        ),
        "T2"
    );
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        value["tasks"],
        serde_json::json!(["T2", " T1 ", "other", "other"])
    );
}
#[test]
fn backlog_removal_cleanup_frozen_scope_and_survivor_identity_cannot_be_reselected() {
    for replacement in [
        "T2,Same,not_started,,2020-02-01,13746cfe-053a-4534-8851-462046c5df88\n",
        "T2,Same,not_started,,2020-02-01,6fc925c4-b131-4e77-9f4d-fde190cad338\nT3,New backlog,backlog,2000-01-01,,5c817d84-c8d4-42a1-8fa5-bc9a15bb2635\n",
    ] {
        let f = WorkspaceFixture::new();
        let header = "task_id,task_name,status,backlogged_date,created_date,task_uuid\nT1,Same,backlog,2000-01-01,,5cf2aa29-b979-4f3e-a65f-7c46b9d08c05\n";
        f.write(
            "tasks/tasks.csv",
            format!(
                "{header}T2,Same,not_started,,2020-02-01,6fc925c4-b131-4e77-9f4d-fde190cad338\n"
            )
            .as_bytes(),
        );
        let plan = preview_maintenance(&f.command(), MaintenanceKind::Dedupe, today())
            .unwrap()
            .plan
            .unwrap();
        f.write(
            "tasks/tasks.csv",
            format!("{header}{replacement}").as_bytes(),
        );
        assert_eq!(
            apply(&f.command(), &plan, &plan.fingerprint, today())
                .unwrap_err()
                .code,
            "stale_plan"
        );
    }
}
#[test]
fn backlog_removal_cleanup_canonical_intent_excludes_labels_and_ordinary_bytes_match_legacy() {
    let f = fixture();
    let snapshot = Snapshot::read(&f.context(), today()).unwrap();
    let options = Options::default();
    let targets = snapshot
        .selected_identities(&needles(&["T1"]), &options)
        .unwrap();
    let document = snapshot.canonical(&options, &targets);
    let legacy = Legacy {
        schema_version: 1,
        workspace_uuid: &snapshot.workspace_uuid,
        actor_id: &snapshot.actor_id,
        execution_date: &snapshot.execution_date,
        options: &options,
        targets: &document.targets,
        tasks: &snapshot.tasks.before,
        habits: &snapshot.habits.before,
        projects: &document.projects,
        integration: &snapshot.integration,
        managed_habits_enabled: snapshot.managed_habits_enabled,
    };
    assert_eq!(
        serde_json::to_vec(&legacy).unwrap(),
        serde_json::to_vec(&document).unwrap()
    );
    let pair = DuplicateReplacement {
        duplicate: snapshot.tasks.identities[0].clone(),
        survivor: snapshot.tasks.identities[1].clone(),
    };
    let mut changed = pair.clone();
    changed.duplicate.task_name = "display label".into();
    changed.survivor.task_name = "another label".into();
    let first = snapshot.canonical_for(
        &options,
        &targets,
        Some(&MaintenanceIntent::BacklogDedupe {
            replacements: vec![pair],
        }),
    );
    let second = snapshot.canonical_for(
        &options,
        &targets,
        Some(&MaintenanceIntent::BacklogDedupe {
            replacements: vec![changed],
        }),
    );
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    let ordinary = preview(&f.command(), &needles(&["T2"]), Options::default(), today()).unwrap();
    assert!(
        serde_json::to_value(ordinary)
            .unwrap()
            .get("maintenance")
            .is_none()
    );
}
