use brain::access::{AccessMode, MachineCapabilityEnvironment, capability_plan};
use brain::config::Config;

use crate::support::{actor, family_id, named_actor, temporary_workspace};

#[test]
fn capability_status_reports_request_availability_and_honest_frontend_enforcement_without_secrets()
{
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec!["notion".to_owned(), "missing".to_owned()],
        allowed_skills: vec!["todo".to_owned(), "missing-skill".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [{
                "name": "notion",
                "url": "https://notion.example.test/mcp",
                "credentials": {"bearer_token": "machine-secret"}
            }]
        }),
    )
    .expect("machine capability environment");
    let plan = capability_plan(&config, &machine).expect("capability plan");

    let status =
        brain::skills::command::format_capability_status(&plan, brain::theme::Theme::dark(false));

    assert!(
        status.contains("notion  requested=yes  available=yes"),
        "{status}"
    );
    assert!(
        status.contains("Claude=strictly-selected  Codex=advisory-only"),
        "{status}"
    );
    assert!(
        status.contains("missing  requested=yes  available=no"),
        "{status}"
    );
    assert!(
        status.contains("Claude=unavailable  Codex=unavailable"),
        "{status}"
    );
    assert!(
        status.contains("todo  requested=yes  available=yes"),
        "{status}"
    );
    assert!(
        status.contains("Claude=advisory-only  Codex=advisory-only"),
        "{status}"
    );
    assert!(!status.contains("machine-secret"), "{status}");
    assert!(!status.contains("https://notion.example.test"), "{status}");
}

#[test]
fn workspace_skill_rendering_is_root_and_actor_aware_without_global_registry_mutation() {
    let (home, workspace) = temporary_workspace();
    let plugin = workspace.root().join(".config/plugins/family-only");
    let extensions = workspace.root().join(".config/extensions");
    std::fs::create_dir_all(&plugin).expect("plugin directory");
    std::fs::create_dir_all(&extensions).expect("extension directory");
    std::fs::write(
        plugin.join("SKILL.md"),
        "# family-only\n<!-- brain:ext family-only:policy -->\n",
    )
    .expect("plugin skill");
    std::fs::write(
        extensions.join("family-only.md"),
        "[family-only:policy]\nSelected family policy\n",
    )
    .expect("plugin extension");
    let registry = home.path().join(".agents/skills");
    std::fs::create_dir_all(&registry).expect("global registry");
    let sentinel = registry.join("global-sentinel");
    std::fs::write(&sentinel, "do not rewrite").expect("global sentinel");
    let before = std::fs::read(&sentinel).expect("sentinel before");
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_skills: vec!["family-only".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "skills": [{"name": "family-only", "path": plugin}]
        }),
    )
    .expect("machine capability environment");
    let plan = capability_plan(&config, &machine).expect("capability plan");
    let pablo = actor();
    let maria = named_actor("maria", "Maria");

    let pablo_report = brain::skills::render_workspace_capabilities(&workspace, &pablo, &plan)
        .expect("Pablo capability render");
    let maria_report = brain::skills::render_workspace_capabilities(&workspace, &maria, &plan)
        .expect("Maria capability render");

    assert_eq!(
        pablo_report.rendered_dir,
        workspace.paths().capability_skills_dir(pablo.user_id())
    );
    assert_eq!(
        maria_report.rendered_dir,
        workspace.paths().capability_skills_dir(maria.user_id())
    );
    assert_ne!(pablo_report.rendered_dir, maria_report.rendered_dir);
    let rendered = std::fs::read_to_string(pablo_report.rendered_dir.join("family-only/SKILL.md"))
        .expect("rendered family skill");
    assert!(rendered.contains("Selected family policy"));
    assert_eq!(std::fs::read(&sentinel).expect("sentinel after"), before);
    assert_eq!(
        std::fs::read_dir(&registry)
            .expect("global registry after")
            .count(),
        1
    );
}
