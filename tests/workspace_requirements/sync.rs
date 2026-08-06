use brain::workspace::{FeatureStatus, RequirementScope, format_requirements, requirements};
use serde_json::{Map, json};

use super::support::{Fixture, feature_status};

#[test]
fn partial_sync_configuration_is_incomplete_instead_of_off() {
    let fixture = Fixture::new(Map::from_iter([(
        "sync".to_owned(),
        json!({"b2_bucket": "configured-without-an-enable-decision"}),
    )]));

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert_eq!(
        feature_status(&health, &RequirementScope::CloudSync),
        FeatureStatus::Incomplete
    );
}

#[test]
fn explicitly_disabled_sync_ignores_stale_partial_fields() {
    let fixture = Fixture::new(Map::from_iter([(
        "sync".to_owned(),
        json!({"enabled": false, "b2_bucket": "stale"}),
    )]));

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert_eq!(
        feature_status(&health, &RequirementScope::CloudSync),
        FeatureStatus::Off
    );
    assert_eq!(
        feature_status(&health, &RequirementScope::SyncWatcher),
        FeatureStatus::Off
    );
}

#[test]
fn complete_sync_configuration_is_ready_without_rendering_credentials() {
    let fixture = Fixture::new(Map::from_iter([(
        "sync".to_owned(),
        json!({
            "enabled": true,
            "b2_bucket": "brain-bucket",
            "b2_path": "",
            "b2_key_id": "sync-key-id",
            "b2_app_key": "sync-app-secret",
            "watch": false
        }),
    )]));

    let health = requirements(&fixture.command).expect("inspect selected workspace");
    let rendered = format_requirements(
        fixture.command.workspace.name(),
        &health,
        brain::theme::Theme::dark(false),
    );

    assert_eq!(
        feature_status(&health, &RequirementScope::CloudSync),
        FeatureStatus::Ready
    );
    assert_eq!(
        feature_status(&health, &RequirementScope::SyncWatcher),
        FeatureStatus::Off
    );
    assert!(!rendered.contains("sync-key-id"), "{rendered}");
    assert!(!rendered.contains("sync-app-secret"), "{rendered}");
}
