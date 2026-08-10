use brain::workspace::{FeatureStatus, RequirementScope, requirements};
use serde_json::Map;

use super::support::{Fixture, feature_status};

/// A workspace with no `tasks/SCHEMA.json` cannot complete a sync at all, so
/// reporting it ready is the failure mode this row exists to prevent.
#[test]
fn a_workspace_without_a_task_schema_document_is_incomplete() {
    let fixture = Fixture::new(Map::new());

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert_eq!(
        feature_status(&health, &RequirementScope::TaskSchema),
        FeatureStatus::Incomplete
    );
}

#[test]
fn a_workspace_declaring_its_task_schema_is_ready() {
    let fixture = Fixture::new(Map::new());
    std::fs::write(
        fixture.command.workspace.root().join("tasks/SCHEMA.json"),
        b"{\"task_schema_version\": 2, \"merge_key\": \"task_uuid\"}\n",
    )
    .expect("task schema document");

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert_eq!(
        feature_status(&health, &RequirementScope::TaskSchema),
        FeatureStatus::Ready
    );
}
