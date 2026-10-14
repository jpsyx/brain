use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::fixture::{WorkspaceFixture, today};
use crate::tasks::removal::projects::clean;
use crate::tasks::removal::snapshot::{ProjectSnapshot, Snapshot};

fn removed() -> BTreeSet<String> {
    BTreeSet::from(["T17".into()])
}

#[test]
fn project_cleanup_removes_archived_and_stale_reverse_links_without_forward_project() {
    let fixture = WorkspaceFixture::new();
    for directory in [
        "projects/team__first",
        "projects/team__second",
        "archive/projects/team__old",
    ] {
        fixture.project(directory);
    }
    let snapshot = Snapshot::read(&fixture.context(), today()).unwrap();
    assert_eq!(snapshot.tasks.parsed.rows[0]["project"], "");
    let after = clean(&snapshot.projects, &removed()).unwrap();
    assert_eq!(after.len(), 3);
    assert!(
        clean(&snapshot.projects, &BTreeSet::new())
            .unwrap()
            .is_empty()
    );
    for (path, bytes) in &after {
        let value: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(value["tasks"], json!([]));
        assert_eq!(value["status"], "in-progress");
        assert_eq!(
            std::fs::read(fixture.workspace.root().join(path)).unwrap(),
            snapshot.projects[path].before
        );
    }
}

#[test]
fn project_cleanup_preserves_unknown_fields_and_survivor_order() {
    let fixture = WorkspaceFixture::new();
    fixture.project("projects/team__first");
    let path = "projects/team__first/.METADATA.json";
    let mut value: Value =
        serde_json::from_slice(&std::fs::read(fixture.workspace.root().join(path)).unwrap())
            .unwrap();
    value["tasks"] = json!(["T8", "T17", "T2", "T8", "T17", " T17 ", "T170"]);
    value["custom"] = json!({"nested": [true, null, 3.5, {"note": "T17"}]});
    fixture.write(path, &serde_json::to_vec(&value).unwrap());
    let snapshot = Snapshot::read(&fixture.context(), today()).unwrap();
    let after = clean(&snapshot.projects, &removed()).unwrap();
    let actual: Value = serde_json::from_slice(&after[path]).unwrap();
    value["tasks"] = json!(["T8", "T2", "T8", " T17 ", "T170"]);
    assert_eq!(actual, value);
}

#[test]
fn project_cleanup_omits_unchanged_bytes_and_absent_tasks() {
    let fixture = WorkspaceFixture::new();
    fixture.project("projects/team__first");
    fixture.project("projects/team__second");
    let first = "projects/team__first/.METADATA.json";
    let second = "projects/team__second/.METADATA.json";
    for (path, tasks) in [(first, Some(json!(["T2"]))), (second, None)] {
        let mut value: Value =
            serde_json::from_slice(&std::fs::read(fixture.workspace.root().join(path)).unwrap())
                .unwrap();
        if let Some(tasks) = tasks {
            value["tasks"] = tasks;
        } else {
            value.as_object_mut().unwrap().remove("tasks");
        }
        fixture.write(
            path,
            &format!("  {}  \n", serde_json::to_string(&value).unwrap()).into_bytes(),
        );
    }
    let snapshot = Snapshot::read(&fixture.context(), today()).unwrap();
    assert!(clean(&snapshot.projects, &removed()).unwrap().is_empty());
    for (path, project) in &snapshot.projects {
        assert_eq!(
            std::fs::read(fixture.workspace.root().join(path)).unwrap(),
            project.before
        );
    }
    assert!(clean(&BTreeMap::new(), &removed()).unwrap().is_empty());
}

#[test]
fn project_cleanup_rejects_malformed_before_images_with_typed_path_error() {
    let path = "archive/projects/team__old/.METADATA.json";
    for before in [
        "{",
        "[]",
        "null",
        r#"{"tasks":null}"#,
        r#"{"tasks":"T17"}"#,
        r#"{"tasks":["T17",7]}"#,
        r#"{"tasks":["T17",null]}"#,
    ] {
        let project = ProjectSnapshot {
            before: before.as_bytes().to_vec(),
            metadata: crate::project::model::Metadata::new(
                "team__old",
                "Example",
                "done",
                "p2",
                "none",
            ),
        };
        let projects = BTreeMap::from([(path.into(), project)]);
        let problem = clean(&projects, &removed()).expect_err(before);
        assert_eq!(problem.code, "invalid_project_metadata");
        assert_eq!(problem.details["path"], path);
    }
}

#[cfg(unix)]
#[test]
fn project_cleanup_uses_snapshot_symlink_exclusion() {
    let fixture = WorkspaceFixture::new();
    fixture.project("projects/team__first");
    let external = fixture.temporary.path().join("external.json");
    let before = br#"{"tasks":["T17"]}"#;
    std::fs::write(&external, before).unwrap();
    let directory = fixture.workspace.root().join("projects/team__linked");
    std::fs::create_dir_all(&directory).unwrap();
    std::os::unix::fs::symlink(&external, directory.join(".METADATA.json")).unwrap();
    let snapshot = Snapshot::read(&fixture.context(), today()).unwrap();
    let after = clean(&snapshot.projects, &removed()).unwrap();
    assert_eq!(
        after.keys().map(String::as_str).collect::<Vec<_>>(),
        ["projects/team__first/.METADATA.json"]
    );
    assert_eq!(std::fs::read(&external).unwrap(), before);
}
