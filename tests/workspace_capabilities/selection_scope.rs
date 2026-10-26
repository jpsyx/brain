use crate::support::family_id;
use brain::{
    access::{AccessMode, MachineCapabilityEnvironment, capability_plan},
    config::Config,
};

#[test]
fn machine_owned_successors_are_never_reclassified_as_bundled() {
    for name in [
        "agenda",
        "projects",
        "process-capture",
        "organize",
        "brain-sync",
    ] {
        let machine = MachineCapabilityEnvironment::from_value(
            family_id(),
            serde_json::json!({"skills": [{"name": name, "path": "/unread/machine"}]}),
        )
        .unwrap();
        let config = Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_skills: vec![name.to_owned()],
            ..Config::default()
        };
        let plan = capability_plan(&config, &machine).unwrap();
        assert!(plan.skills.available_names().is_empty(), "{name}");
        assert!(
            plan.skills
                .unavailable_reason(name)
                .unwrap()
                .contains("ownership collision")
        );
    }
}

#[test]
fn current_todo_and_triage_selections_do_not_grant_delegated_workflows() {
    let machine =
        MachineCapabilityEnvironment::from_value(family_id(), serde_json::json!({})).unwrap();
    for selected in ["todo", "triage", "second-brain"] {
        let config = Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_skills: vec![selected.to_owned()],
            ..Config::default()
        };
        let plan = capability_plan(&config, &machine).unwrap();
        assert_eq!(plan.skills.available_names(), [selected]);
        for delegated in [
            "agenda",
            "projects",
            "process-capture",
            "organize",
            "brain-sync",
        ] {
            assert!(
                plan.skills
                    .unavailable_reason(delegated)
                    .unwrap()
                    .contains("not selected")
            );
        }
    }
}

#[test]
fn default_compatibility_routes_have_selected_successors() {
    let machine =
        MachineCapabilityEnvironment::from_value(family_id(), serde_json::json!({})).unwrap();
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        ..Config::default()
    };
    let plan = capability_plan(&config, &machine).unwrap();
    for (route, owner) in [
        ("todo agenda", "agenda"),
        ("todo plan-day", "agenda"),
        ("second-brain add-project", "projects"),
        ("second-brain complete-project", "projects"),
        ("second-brain archive-project", "projects"),
        ("second-brain extract-ip", "projects"),
        ("second-brain process-capture", "process-capture"),
        ("second-brain add-resource", "organize"),
        ("second-brain find", "organize"),
        ("second-brain reindex", "organize"),
        ("second-brain cloud-sync", "brain-sync"),
        ("second-brain resolve-conflicts", "brain-sync"),
    ] {
        assert!(plan.skills.available_names().contains(&owner), "{route}");
        assert_eq!(plan.skills.unavailable_reason(owner), None, "{route}");
    }
    for external in ["brain-knowledge-capture", "article-summarizer"] {
        assert!(
            plan.skills
                .unavailable_reason(external)
                .unwrap()
                .contains("not selected")
        );
    }
}
