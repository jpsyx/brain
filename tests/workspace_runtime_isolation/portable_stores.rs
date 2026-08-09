use std::collections::BTreeMap;

use brain::personalization::persona::Persona;
use brain::personalization::tags::TagStyle;
use brain::workspace::{RegistryStore, WorkspaceName};
use serde_json::Value;

use crate::support::Fixture;

#[test]
fn selected_context_silos_env_config_and_personalization_after_default_changes() {
    let fixture = Fixture::new();

    assert_eq!(
        brain::env::resolve_one(&fixture.personal, "claude_cmd"),
        Some("personal".to_owned())
    );
    assert_eq!(
        brain::env::resolve_one(&fixture.family, "claude_cmd"),
        Some("family".to_owned())
    );

    brain::settings::set(&fixture.personal.workspace, "day_rollover_hour", "3")
        .expect("write personal config");
    brain::settings::set(&fixture.family.workspace, "day_rollover_hour", "8")
        .expect("write family config");
    assert_eq!(
        brain::settings::resolve_one(&fixture.personal.workspace, "day_rollover_hour"),
        Some("3".to_owned())
    );
    assert_eq!(
        brain::settings::resolve_one(&fixture.family.workspace, "day_rollover_hour"),
        Some("8".to_owned())
    );

    let personal = Persona {
        name: "Personal".to_owned(),
        tag_styles: BTreeMap::from([(
            "shared".to_owned(),
            TagStyle {
                emoji: "P".to_owned(),
                label: "personal".to_owned(),
            },
        )]),
        ..Persona::default()
    };
    let family = Persona {
        name: "Family".to_owned(),
        tag_styles: BTreeMap::from([(
            "shared".to_owned(),
            TagStyle {
                emoji: "F".to_owned(),
                label: "family".to_owned(),
            },
        )]),
        ..Persona::default()
    };
    brain::personalization::store::save_persona(
        &fixture.personal.workspace,
        fixture.personal.workspace.local_user_id(),
        &personal,
    )
    .expect("write personal personalization");
    brain::personalization::store::save_persona(
        &fixture.family.workspace,
        fixture.family.workspace.local_user_id(),
        &family,
    )
    .expect("write family personalization");
    assert_eq!(
        brain::personalization::load_tag_styles(&fixture.personal.workspace).label("shared"),
        "P personal"
    );
    assert_eq!(
        brain::personalization::load_tag_styles(&fixture.family.workspace).label("shared"),
        "F family"
    );

    let before_family = std::fs::read(fixture.family.workspace.root().join(".config/config.json"))
        .expect("family config bytes");
    brain::env::set(&fixture.personal, "claude_cmd", "updated").expect("write personal env");
    brain::settings::set(&fixture.personal.workspace, "day_rollover_hour", "4")
        .expect("rewrite personal config");
    assert_eq!(
        std::fs::read(fixture.family.workspace.root().join(".config/config.json"))
            .expect("family config bytes"),
        before_family
    );

    let mut registry = RegistryStore::load_from(fixture.store.path()).expect("load registry");
    registry.set_default("family").expect("change default");
    fixture.store.replace(&registry).expect("persist default");
    assert_eq!(
        brain::env::resolve_one(&fixture.personal, "claude_cmd"),
        Some("updated".to_owned())
    );
    assert_eq!(
        brain::env::resolve_one(&fixture.family, "claude_cmd"),
        Some("family".to_owned())
    );
    assert_eq!(
        RegistryStore::load_from(fixture.store.path())
            .expect("load registry")
            .workspaces[&WorkspaceName::parse("family").expect("name")]
            .env["claude_cmd"],
        Value::String("family".to_owned())
    );
}
