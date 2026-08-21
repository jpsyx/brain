use super::*;

fn sample() -> Persona {
    Persona {
        name: "Pablo".to_owned(),
        role: "CEO".to_owned(),
        works_for: "Avandar".to_owned(),
        ..Persona::default()
    }
}

#[test]
fn summary_block_emits_stable_keyed_lines() {
    let block = summary_block(&sample());
    // namespaces falls back to the generic defaults when unset.
    assert_eq!(
        block,
        "name: Pablo\nrole: CEO\nworks_for: Avandar\nnamespaces: work, personal"
    );
}

#[test]
fn summary_block_shows_configured_namespaces() {
    let mut p = sample();
    p.namespaces = vec![
        "avandar".to_owned(),
        "personal".to_owned(),
        "pole".to_owned(),
    ];
    assert!(summary_block(&p).ends_with("namespaces: avandar, personal, pole"));
}

#[test]
fn summary_block_shows_unset_for_empty_fields() {
    let block = summary_block(&Persona::default());
    assert_eq!(
        block,
        "name: (unset)\nrole: (unset)\nworks_for: (unset)\nnamespaces: work, personal"
    );
}

#[test]
fn a_persona_block_names_its_owner_and_marks_the_local_person() {
    assert!(persona_block("pablo", &sample(), true).starts_with("user: pablo (this machine)\n"));
    assert!(persona_block("sam", &sample(), false).starts_with("user: sam\nname: Pablo"));
}

#[test]
fn the_roster_block_covers_every_member_including_unpersonalized_ones() {
    let personas = Personas::parse(
        r#"{"schema_version": 2, "personas": {"pablo": {"name": "Pablo", "role": "CEO"}}}"#,
        "pablo",
    );

    let block = roster_block(&personas, &["pablo", "sam"], "pablo");

    assert!(block.contains("user: pablo (this machine)"), "{block}");
    assert!(block.contains("role: CEO"), "{block}");
    // Sam has no entry yet, but a skill must still learn they exist.
    assert!(block.contains("user: sam"), "{block}");
    assert!(block.contains("name: (unset)"), "{block}");
    assert_eq!(block.matches("user: ").count(), 2, "{block}");
}

#[test]
fn the_roster_block_always_includes_the_local_person() {
    // A workspace with no portable user store still reports its own reader.
    let block = roster_block(&Personas::default(), &[], "pablo");

    assert!(block.starts_with("user: pablo (this machine)"), "{block}");
}

#[test]
fn a_persona_stored_for_someone_off_the_roster_is_still_listed() {
    // Removing a member should not make their stored persona invisible.
    let personas = Personas::parse(
        r#"{"schema_version": 2, "personas": {"ghost": {"role": "founder"}}}"#,
        "pablo",
    );

    let block = roster_block(&personas, &["pablo"], "pablo");

    assert!(block.contains("user: ghost"), "{block}");
}

#[test]
fn get_field_reads_known_fields_and_none_for_empty_or_unknown() {
    let p = sample();
    assert_eq!(get_field(&p, "role").as_deref(), Some("CEO"));
    assert_eq!(get_field(&Persona::default(), "role"), None);
    assert_eq!(get_field(&p, "bogus"), None);
}

#[test]
fn set_field_updates_known_fields() {
    let mut p = Persona::default();
    set_field(&mut p, "role", "student").unwrap();
    set_field(&mut p, "works_for", "myself").unwrap();
    assert_eq!(p.role, "student");
    assert_eq!(p.works_for, "myself");
}

#[test]
fn set_field_rejects_unknown_field() {
    let mut p = Persona::default();
    assert!(set_field(&mut p, "nope", "x").is_err());
}

#[test]
fn set_field_points_tag_styles_at_edit() {
    let mut p = Persona::default();
    let err = set_field(&mut p, "tag_styles", "{}").unwrap_err();
    assert!(err.to_string().contains("edit"));
}

#[test]
fn an_unknown_user_is_rejected_with_the_members_the_workspace_knows() {
    let roster = ["pablo".to_owned(), "sam".to_owned()];

    assert!(validate_user(&roster, "pablo").is_ok());
    let error = validate_user(&roster, "ghost").unwrap_err().to_string();
    assert!(error.contains("unknown user `ghost`"), "{error}");
    assert!(error.contains("pablo, sam"), "{error}");
}

#[test]
fn a_workspace_with_no_portable_users_accepts_any_id() {
    // Legacy, pre-migration workspaces must still be personalizable.
    assert!(validate_user(&[], "pablo").is_ok());
}
