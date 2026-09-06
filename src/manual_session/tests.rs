use super::*;

#[test]
fn a_manual_session_name_is_trimmed_before_storage() {
    let name = ManualSessionName::parse("  Project Atlas  ", &[]).unwrap();
    assert_eq!(name.as_str(), "Project Atlas");
}

#[test]
fn blank_and_case_insensitive_duplicate_names_are_rejected() {
    assert_eq!(
        ManualSessionName::parse("   ", &[]),
        Err(ManualSessionNameError::Blank)
    );
    assert_eq!(
        ManualSessionName::parse(" brain ", &["Brain".to_owned()]),
        Err(ManualSessionNameError::Duplicate)
    );
}

#[test]
fn manual_session_ids_are_non_blank_and_new_ids_are_distinct() {
    assert!(ManualSessionId::parse("   ").is_none());
    assert_eq!(
        ManualSessionId::parse(" manual-id ").unwrap().as_str(),
        "manual-id"
    );
    assert_ne!(ManualSessionId::new(), ManualSessionId::new());
}

#[test]
fn roles_round_trip_through_their_stable_database_values() {
    assert_eq!(ManualSessionRole::Main.as_str(), "main");
    assert_eq!(
        ManualSessionRole::parse("additional"),
        Some(ManualSessionRole::Additional)
    );
    assert_eq!(ManualSessionRole::parse("unknown"), None);
}

#[test]
fn record_constructors_set_the_main_and_additional_invariants() {
    let main_id = ManualSessionId::parse("main-id").unwrap();
    let main_session = crate::agent::AgentSession::new("main-native").unwrap();
    let main = ManualSessionRecord::main(main_id.clone(), main_session.clone());
    assert_eq!(main.id(), &main_id);
    assert_eq!(main.agent_session(), &main_session);
    assert_eq!(main.name(), &ManualSessionName::main());
    assert_eq!(main.position(), 0);
    assert_eq!(main.role(), ManualSessionRole::Main);

    let additional_id = ManualSessionId::parse("additional-id").unwrap();
    let additional_session = crate::agent::AgentSession::new("additional-native").unwrap();
    let additional_name = ManualSessionName::parse("Atlas", &["Brain".to_owned()]).unwrap();
    let additional = ManualSessionRecord::additional(
        additional_id.clone(),
        additional_session.clone(),
        additional_name.clone(),
        2,
    );
    assert_eq!(additional.id(), &additional_id);
    assert_eq!(additional.agent_session(), &additional_session);
    assert_eq!(additional.name(), &additional_name);
    assert_eq!(additional.position(), 2);
    assert_eq!(additional.role(), ManualSessionRole::Additional);
}
