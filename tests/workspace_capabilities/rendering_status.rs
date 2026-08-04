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

    let status = brain::skills::command::format_capability_status(
        &plan,
        "claude",
        brain::theme::Theme::dark(false),
    );

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
fn capability_status_downgrades_claude_for_an_indirect_configured_command() {
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_mcps: vec!["notion".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "mcps": [{"name": "notion", "url": "https://notion.example.test/mcp"}]
        }),
    )
    .expect("machine capability environment");
    let plan = capability_plan(&config, &machine).expect("capability plan");

    let status = brain::skills::command::format_capability_status(
        &plan,
        "sh -c 'exec claude'",
        brain::theme::Theme::dark(false),
    );

    assert!(
        status.contains("Claude=advisory-only  Codex=advisory-only"),
        "{status}"
    );
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

#[test]
fn machine_skill_rendering_reads_the_exact_configured_directory_not_a_named_sibling() {
    let (_home, workspace) = temporary_workspace();
    let sources = workspace.root().join("machine-skills");
    let configured = sources.join("configured-source");
    let misleading = sources.join("family-only");
    std::fs::create_dir_all(&configured).expect("configured source");
    std::fs::create_dir_all(&misleading).expect("misleading sibling");
    std::fs::write(configured.join("SKILL.md"), "# exact configured source\n")
        .expect("configured skill");
    std::fs::write(misleading.join("SKILL.md"), "# wrong named sibling\n")
        .expect("misleading skill");
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_skills: vec!["family-only".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "skills": [{"name": "family-only", "path": configured}]
        }),
    )
    .expect("machine capability environment");
    let plan = capability_plan(&config, &machine).expect("capability plan");

    let report = brain::skills::render_workspace_capabilities(&workspace, &actor(), &plan)
        .expect("exact skill render");
    let rendered = std::fs::read_to_string(report.rendered_dir.join("family-only/SKILL.md"))
        .expect("rendered skill");

    assert!(rendered.contains("exact configured source"), "{rendered}");
    assert!(!rendered.contains("wrong named sibling"), "{rendered}");
}

#[cfg(unix)]
#[test]
fn machine_skill_symlink_paths_and_entries_are_unavailable_without_traversal() {
    use std::os::unix::fs::symlink;

    let (_home, workspace) = temporary_workspace();
    let outside = workspace.root().join("outside-skill");
    std::fs::create_dir_all(&outside).expect("outside skill");
    std::fs::write(outside.join("SKILL.md"), "# outside\n").expect("outside skill file");

    let linked_root = workspace.root().join("linked-skill");
    symlink(&outside, &linked_root).expect("linked skill root");
    let root_config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_skills: vec!["linked".to_owned()],
        ..Config::default()
    };
    let root_machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({"skills": [{"name": "linked", "path": linked_root}]}),
    )
    .expect("linked root machine environment");
    let root_plan = capability_plan(&root_config, &root_machine).expect("linked root plan");
    assert!(root_plan.skills.available_names().is_empty());
    assert!(
        root_plan
            .skills
            .unavailable_reason("linked")
            .is_some_and(|reason| reason.contains("symlink"))
    );

    let cyclic = workspace.root().join("cyclic-skill");
    std::fs::create_dir_all(cyclic.join("scripts")).expect("cyclic skill");
    std::fs::write(cyclic.join("SKILL.md"), "# cyclic\n").expect("cyclic skill file");
    symlink(&cyclic, cyclic.join("scripts/cycle")).expect("skill cycle");
    let cycle_config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_skills: vec!["cyclic".to_owned()],
        ..Config::default()
    };
    let cycle_machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({"skills": [{"name": "cyclic", "path": cyclic}]}),
    )
    .expect("cyclic machine environment");
    let cycle_plan = capability_plan(&cycle_config, &cycle_machine).expect("cyclic plan");

    assert!(cycle_plan.skills.available_names().is_empty());
    assert!(
        cycle_plan
            .skills
            .unavailable_reason("cyclic")
            .is_some_and(|reason| reason.contains("symlink"))
    );
}

#[cfg(unix)]
#[test]
fn machine_skill_parent_symlink_cannot_be_retargeted_into_an_available_source() {
    use std::os::unix::fs::symlink;

    let (_home, workspace) = temporary_workspace();
    let first_parent = workspace.root().join("first-parent");
    let second_parent = workspace.root().join("second-parent");
    for parent in [&first_parent, &second_parent] {
        std::fs::create_dir_all(parent.join("skill")).expect("machine skill parent");
        std::fs::write(parent.join("skill/SKILL.md"), "# machine skill\n")
            .expect("machine skill file");
    }
    let configured_parent = workspace.root().join("configured-parent");
    symlink(&first_parent, &configured_parent).expect("configured parent symlink");
    let config = Config {
        access_mode: AccessMode::WorkspaceOnly,
        allowed_skills: vec!["machine-only".to_owned()],
        ..Config::default()
    };
    let machine = MachineCapabilityEnvironment::from_value(
        family_id(),
        serde_json::json!({
            "skills": [{"name": "machine-only", "path": configured_parent.join("skill")}]
        }),
    )
    .expect("machine capability environment");
    std::fs::remove_file(&configured_parent).expect("remove first parent link");
    symlink(&second_parent, &configured_parent).expect("retarget configured parent link");

    let plan = capability_plan(&config, &machine).expect("capability plan");

    assert!(plan.skills.available_names().is_empty());
    assert!(
        plan.skills
            .unavailable_reason("machine-only")
            .is_some_and(|reason| reason.contains("symlink"))
    );
}
