#[test]
fn logical_access_appends_only_the_selected_legacy_successors() {
    let input = ["todo", "second-brain", "custom", "agenda", "todo"].map(str::to_owned);
    assert_eq!(
        super::expand_legacy_names(&input),
        [
            "todo",
            "second-brain",
            "custom",
            "agenda",
            "projects",
            "process-capture",
            "organize",
            "brain-sync"
        ]
    );
    assert_eq!(
        super::expand_legacy_names(&["custom".to_owned()]),
        ["custom"]
    );
    assert_eq!(
        super::expand_legacy_names(&["projects".to_owned()]),
        ["projects"]
    );
    assert_eq!(
        super::expand_legacy_names(&["todo".to_owned()]),
        ["todo", "agenda"]
    );
    assert_eq!(
        super::expand_legacy_names(&["second-brain".to_owned()]),
        [
            "second-brain",
            "projects",
            "process-capture",
            "organize",
            "brain-sync"
        ]
    );
    assert!(super::expand_legacy_names(&[]).is_empty());
}

#[test]
fn explicit_legacy_projection_is_narrow_idempotent_and_records_original_values() {
    use serde_json::json;
    for (before, after) in [
        (
            json!(["contacts", "second-brain", "todo", "triage"]),
            json!([
                "contacts",
                "second-brain",
                "todo",
                "triage",
                "projects",
                "process-capture",
                "organize",
                "brain-sync",
                "agenda"
            ]),
        ),
        (
            json!(["todo", "custom-skill"]),
            json!(["todo", "custom-skill", "agenda"]),
        ),
        (
            json!(["second-brain"]),
            json!([
                "second-brain",
                "projects",
                "process-capture",
                "organize",
                "brain-sync"
            ]),
        ),
        (
            json!(["projects", "second-brain", "agenda", "todo"]),
            json!([
                "projects",
                "second-brain",
                "agenda",
                "todo",
                "process-capture",
                "organize",
                "brain-sync"
            ]),
        ),
        (
            json!(["TODO", "Custom"]),
            json!(["TODO", "Custom", "agenda"]),
        ),
    ] {
        let config = json!({"allowed_skills": before, "other": {"keep": [null, 42]}});
        let patch = super::project_allowed_skills(&config).unwrap().unwrap();
        assert_eq!(patch.before, Some(before));
        assert_eq!(patch.after, Some(after.clone()));
        assert_eq!(patch.field, "allowed_skills");
        assert_eq!(
            patch.relative_path,
            std::path::Path::new(".config/config.json")
        );
        let mut migrated = config.clone();
        migrated["allowed_skills"] = after;
        assert!(super::project_allowed_skills(&migrated).unwrap().is_none());
        assert_eq!(migrated["other"], config["other"]);
    }
    for config in [
        json!({}),
        json!({"allowed_skills": []}),
        json!({"allowed_skills": ["custom"]}),
        json!({"allowed_skills": ["projects"]}),
    ] {
        assert!(super::project_allowed_skills(&config).unwrap().is_none());
    }
    for config in [
        json!(null),
        json!([]),
        json!({"allowed_skills": null}),
        json!({"allowed_skills": [42]}),
    ] {
        assert!(super::project_allowed_skills(&config).is_err());
    }
}

#[test]
fn down_reverses_only_recorded_introductions_preserving_later_custom_additions() {
    use serde_json::json;
    let before = json!({"allowed_skills": ["todo", "Custom"], "keep": true});
    let patch = super::project_allowed_skills(&before).unwrap().unwrap();
    let current = json!({"allowed_skills": ["todo", "Custom", "agenda", "later"], "keep": false});
    let reversed = super::reverse_allowed_skills(&current, &[patch])
        .unwrap()
        .unwrap();
    assert_eq!(reversed.before, Some(current["allowed_skills"].clone()));
    assert_eq!(reversed.after, Some(json!(["todo", "Custom", "later"])));
    assert_eq!(current["keep"], false);
    assert!(
        super::reverse_allowed_skills(&json!({}), &[])
            .unwrap()
            .is_none()
    );
    assert!(
        super::reverse_allowed_skills(&json!({"allowed_skills": []}), &[])
            .unwrap()
            .is_none()
    );
}

#[test]
fn down_refuses_canonical_only_or_narrowed_umbrella_authority_and_forged_history() {
    use serde_json::json;
    let patch = super::project_allowed_skills(&json!({"allowed_skills": ["todo"]}))
        .unwrap()
        .unwrap();
    for names in [
        json!(["projects"]),
        json!(["agenda"]),
        json!(["todo"]),
        json!(["second-brain", "projects"]),
        json!(["projects", "process-capture", "organize", "brain-sync"]),
    ] {
        let error = super::reverse_allowed_skills(
            &json!({"allowed_skills": names}),
            std::slice::from_ref(&patch),
        )
        .unwrap_err();
        assert!(error.to_string().contains("equivalent scope"), "{error}");
    }
    let mut forged = patch;
    forged.after = Some(json!(["todo", "agenda", "custom"]));
    assert!(
        super::reverse_allowed_skills(
            &json!({"allowed_skills": ["todo", "agenda", "custom"]}),
            &[forged]
        )
        .is_err()
    );
}

#[test]
fn down_does_not_remove_preexisting_canonical_names() {
    use serde_json::json;
    let before = json!({"allowed_skills": ["second-brain", "projects", "custom"]});
    let patch = super::project_allowed_skills(&before).unwrap().unwrap();
    let current = json!({"allowed_skills": patch.after});
    let reverse = super::reverse_allowed_skills(&current, &[patch])
        .unwrap()
        .unwrap();
    assert_eq!(reverse.after, Some(before["allowed_skills"].clone()));
}

#[test]
fn receiptless_legacy_scope_validation_preserves_legacy_and_plugin_names() {
    use serde_json::json;
    for names in [
        json!([]),
        json!(["Custom"]),
        json!(["TODO", "second-brain", "Custom"]),
        json!(["todo", "agenda", "Custom"]),
        json!([
            "second-brain",
            "projects",
            "process-capture",
            "organize",
            "brain-sync"
        ]),
    ] {
        super::validate_legacy_allowed_skills(&json!({"allowed_skills": names})).unwrap();
    }
    super::validate_legacy_allowed_skills(&json!({})).unwrap();
    for names in [
        json!(["agenda"]),
        json!(["projects"]),
        json!(["process-capture"]),
        json!(["organize"]),
        json!(["brain-sync"]),
        json!(["second-brain", "projects"]),
        json!(["projects", "process-capture", "organize", "brain-sync"]),
    ] {
        let error =
            super::validate_legacy_allowed_skills(&json!({"allowed_skills": names})).unwrap_err();
        assert!(error.to_string().contains("equivalent scope"), "{error}");
    }
}

#[test]
fn machine_selection_preflight_refuses_all_new_bundled_name_claims() {
    for name in [
        "agenda",
        "projects",
        "process-capture",
        "organize",
        "brain-sync",
    ] {
        let machine = crate::access::MachineCapabilityEnvironment::from_value(
            crate::workspace::WorkspaceId::new(),
            serde_json::json!({"skills": [{"name": name, "path": "/unread/machine"}]}),
        )
        .unwrap();
        assert!(
            crate::access::preflight_bundled_skill_ownership(&machine)
                .unwrap_err()
                .to_string()
                .contains(name)
        );
    }
    let machine = crate::access::MachineCapabilityEnvironment::from_value(
        crate::workspace::WorkspaceId::new(),
        serde_json::json!({"skills": [{"name": "custom", "path": "/unread/machine"}]}),
    )
    .unwrap();
    crate::access::preflight_bundled_skill_ownership(&machine).unwrap();
}
