use brain::workspace::{PromptMetadata, RequirementScope, format_requirements, requirements};
use serde_json::{Map, Value};

use super::support::Fixture;

#[test]
fn requirements_expose_prompt_secrecy_without_rendering_values() {
    let fixture = Fixture::with_receiver(
        Map::from_iter([
            (
                "twilio_auth_token".to_owned(),
                Value::String("credential-that-must-not-render".to_owned()),
            ),
            (
                "twilio_from_number".to_owned(),
                Value::String("+15551234567".to_owned()),
            ),
        ]),
        true,
    );

    let health = requirements(&fixture.command).expect("inspect selected workspace");
    let sms = health
        .iter()
        .find(|requirement| requirement.scope() == &RequirementScope::Sms)
        .expect("SMS requirement");

    assert!(
        sms.prompts()
            .contains(&PromptMetadata::secret("Twilio auth token"))
    );
    let rendered = format_requirements(
        fixture.command.workspace.name(),
        &health,
        brain::theme::Theme::dark(false),
    );
    assert!(
        !rendered.contains("credential-that-must-not-render"),
        "{rendered}"
    );
    assert!(!rendered.contains("+15551234567"), "{rendered}");
    assert!(rendered.contains("Workspace brain"), "{rendered}");
    assert!(rendered.contains("incomplete"), "{rendered}");
}
