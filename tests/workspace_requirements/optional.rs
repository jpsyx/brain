use brain::workspace::{FeatureStatus, RequirementScope, requirements};
use serde_json::Map;

use super::support::{Fixture, feature_status};

#[test]
fn disabled_optional_features_are_off_without_readiness_errors() {
    let fixture = Fixture::new(Map::new());

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    for scope in [
        RequirementScope::CloudSync,
        RequirementScope::SyncWatcher,
        RequirementScope::Receiver,
        RequirementScope::Sms,
        RequirementScope::Email,
        RequirementScope::TriageHabits,
        RequirementScope::Linear,
        RequirementScope::PersonalizationRole,
        RequirementScope::PersonalizationOrganization,
        RequirementScope::PersonalizationTagStyles,
    ] {
        assert_eq!(
            feature_status(&health, &scope),
            FeatureStatus::Off,
            "{scope:?}"
        );
    }
    assert_eq!(
        feature_status(&health, &RequirementScope::BrowserViews),
        FeatureStatus::Ready
    );
    assert_eq!(
        feature_status(&health, &RequirementScope::WebViews),
        FeatureStatus::Ready
    );
}

#[test]
fn enabled_triage_without_both_managed_rows_is_incomplete() {
    let fixture = Fixture::new(Map::new());
    fixture.write_config(
        r#"{"access_mode":"unrestricted","enable_triage_habits":true,"daily_triage_name_pattern":"["}"#,
    );
    std::fs::write(
        fixture.command.workspace.root().join("tasks/habits.csv"),
        "task_id,task_name,status,assigned_to,system_key\nH1,Morning Triage,not_started,pablo,brain.triage.daily\n",
    )
    .expect("partial managed habits");

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert_eq!(
        feature_status(&health, &RequirementScope::TriageHabits),
        FeatureStatus::Incomplete
    );
    assert_eq!(
        feature_status(&health, &RequirementScope::TriageModal),
        FeatureStatus::Incomplete
    );
}

#[test]
fn workspace_only_capabilities_report_each_requested_missing_item() {
    let fixture = Fixture::new(Map::new());
    fixture.write_config(
        r#"{
          "access_mode":"workspace_only",
          "allowed_mcps":["linear"],
          "allowed_skills":["custom-review"],
          "enable_triage_habits":false
        }"#,
    );

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert_eq!(
        feature_status(&health, &RequirementScope::AccessPolicy),
        FeatureStatus::Ready
    );
    assert_eq!(
        feature_status(&health, &RequirementScope::Mcp("linear".to_owned())),
        FeatureStatus::Incomplete
    );
    assert_eq!(
        feature_status(
            &health,
            &RequirementScope::Skill("custom-review".to_owned())
        ),
        FeatureStatus::Incomplete
    );
}

#[test]
fn workspace_only_core_skills_are_not_optional_setup_rows() {
    let fixture = Fixture::new(Map::new());
    fixture.write_config(r#"{"access_mode":"workspace_only","enable_triage_habits":false}"#);

    let health = requirements(&fixture.command).expect("inspect selected workspace");

    assert!(
        health
            .iter()
            .all(|requirement| !matches!(requirement.scope(), RequirementScope::Skill(_))),
        "bundled core skills are not optional machine setup: {health:?}"
    );
}
