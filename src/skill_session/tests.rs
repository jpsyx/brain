//! Unit tests for the pure skill-session model: what the workspace offers,
//! what a running session hides, and how a malformed definition degrades.

use serde_json::json;

use super::*;

fn custom(configured: &serde_json::Value) -> Vec<SkillSessionSpec> {
    available(false, Some(configured))
}

#[test]
fn daily_triage_is_a_builtin_offered_only_while_the_check_is_enabled() {
    let enabled = available(true, None);
    assert_eq!(enabled.len(), 1);
    assert_eq!(enabled[0].key, SkillSessionKey::DailyTriage);
    assert_eq!(enabled[0].title, "Daily triage");
    assert_eq!(enabled[0].prompt, "/triage");
    assert_eq!(enabled[0].command_label, "Run daily triage");

    assert!(available(false, None).is_empty());
}

#[test]
fn the_builtin_leads_the_configured_sessions_in_declaration_order() {
    let specs = available(
        true,
        Some(&json!([
            {"title": "Email triage", "prompt": "/email-triage", "command_label": "Run email triage"},
            {"title": "Weekly review", "prompt": "/triage weekly", "command_label": "Run weekly review"},
        ])),
    );

    let keys: Vec<_> = specs.iter().map(|spec| spec.key).collect();
    assert_eq!(
        keys,
        vec![
            SkillSessionKey::DailyTriage,
            SkillSessionKey::Custom(0),
            SkillSessionKey::Custom(1),
        ]
    );
    assert_eq!(specs[1].title, "Email triage");
    assert_eq!(specs[1].prompt, "/email-triage");
    assert_eq!(specs[1].command_label, "Run email triage");
}

#[test]
fn a_configured_session_needs_only_a_prompt_and_borrows_the_rest() {
    let specs = custom(&json!([{"prompt": "  /email-triage  "}]));

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].prompt, "/email-triage");
    assert_eq!(specs[0].title, "/email-triage");
    assert_eq!(specs[0].command_label, "Run /email-triage");
}

#[test]
fn a_configured_session_without_a_label_names_itself_from_its_title() {
    let specs = custom(&json!([{"prompt": "/email-triage", "title": "Email triage"}]));

    assert_eq!(specs[0].command_label, "Run Email triage");
}

#[test]
fn malformed_definitions_are_dropped_without_shifting_the_survivors_keys() {
    // A promptless entry, a non-object, and a blank prompt are all unusable;
    // dropping them must not renumber the entries a user can still run, or a
    // palette row would point at a different session after an edit.
    let specs = custom(&json!([
        {"title": "No prompt"},
        "not an object",
        {"prompt": "   "},
        {"prompt": "/email-triage", "title": "Email triage"},
    ]));

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].key, SkillSessionKey::Custom(3));
}

#[test]
fn a_non_array_configuration_offers_nothing_rather_than_failing() {
    assert!(custom(&json!({"prompt": "/email-triage"})).is_empty());
    assert!(custom(&json!("/email-triage")).is_empty());
    assert!(custom(&json!(null)).is_empty());
}

#[test]
fn a_running_session_is_no_longer_runnable_but_its_siblings_still_are() {
    let specs = available(
        true,
        Some(&json!([{"prompt": "/email-triage", "title": "Email triage"}])),
    );

    let offered: Vec<_> = runnable(&specs, &[SkillSessionKey::DailyTriage])
        .into_iter()
        .map(|spec| spec.key)
        .collect();

    assert_eq!(offered, vec![SkillSessionKey::Custom(0)]);
    assert!(
        runnable(
            &specs,
            &[SkillSessionKey::DailyTriage, SkillSessionKey::Custom(0)]
        )
        .is_empty()
    );
}
