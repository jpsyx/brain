use std::collections::BTreeSet;

use brain::workspace::{
    FeatureStatus, RegistryStore, RequirementScope, WorkspaceId, WorkspaceName, WorkspaceRecord,
    format_requirements, requirements,
};
use serde_json::{Map, json};

use super::support::{Fixture, feature_status};

#[test]
fn inspection_never_inherits_configuration_from_a_peer_workspace() {
    let fixture = Fixture::new(Map::new());
    let mut registry =
        RegistryStore::load_from(fixture.command.registry_store.path()).expect("load registry");
    let peer = WorkspaceName::parse("family").expect("peer name");
    registry.workspaces.insert(
        peer,
        WorkspaceRecord {
            workspace_id: WorkspaceId::parse("4bf502e6-0a06-45ef-9f49-4599c8900db6")
                .expect("peer UUID"),
            root: fixture.command.workspace.root().with_extension("family"),
            aliases: BTreeSet::new(),
            local_user_id: "pablo".to_owned(),
            receiver_enabled: true,
            env: Map::from_iter([(
                "sync".to_owned(),
                json!({
                    "enabled": true,
                    "b2_bucket": "peer-only",
                    "b2_path": "peer",
                    "b2_key_id": "peer-key",
                    "b2_app_key": "peer-secret"
                }),
            )]),
        },
    );
    fixture
        .command
        .registry_store
        .replace(&registry)
        .expect("store peer workspace");

    let health = requirements(&fixture.command).expect("inspect selected workspace");
    let rendered = format_requirements(
        fixture.command.workspace.name(),
        &health,
        brain::theme::Theme::dark(false),
    );

    assert_eq!(
        feature_status(&health, &RequirementScope::CloudSync),
        FeatureStatus::Off
    );
    assert!(!rendered.contains("peer-only"), "{rendered}");
    assert!(!rendered.contains("peer-secret"), "{rendered}");
}
