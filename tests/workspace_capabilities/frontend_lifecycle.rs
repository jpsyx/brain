use std::sync::Arc;

use brain::access::{AccessMode, MachineCapabilityEnvironment, capability_plan};
use brain::agent::{AgentKind, AgentSession, LaunchRequest, SessionPlan};
use brain::config::Config;

use crate::support::{
    actor, family_id, launch_spec, launch_with_spawn_failure, temporary_workspace,
};

#[test]
fn claude_removes_stale_runtime_temps_and_codex_wrappers_before_launch() {
    let (_home, workspace) = temporary_workspace();
    let capabilities = workspace.paths().capabilities_dir();
    let stale_temp = capabilities.join(".claude-mcp-abandoned.tmp");
    let stale_wrapper = capabilities.join("codex-mcp/stale.sh");
    std::fs::create_dir_all(stale_wrapper.parent().expect("wrapper parent"))
        .expect("stale wrapper directory");
    std::fs::write(&stale_temp, "stale-secret").expect("stale Claude temp");
    std::fs::write(&stale_wrapper, "stale wrapper").expect("stale Codex wrapper");
    let plan = capability_plan(
        &Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_mcps: vec!["notion".to_owned()],
            allowed_skills: Vec::new(),
            ..Config::default()
        },
        &MachineCapabilityEnvironment::from_value(
            family_id(),
            serde_json::json!({
                "mcps": [{"name": "notion", "url": "https://example.test/mcp"}]
            }),
        )
        .expect("machine capability environment"),
    )
    .expect("capability plan");
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(plan);

    launch_spec(AgentKind::Claude, "claude", &request).expect("Claude launch spec");

    assert!(!stale_temp.exists());
    assert!(!stale_wrapper.exists());
    assert!(!capabilities.join("codex-mcp").exists());
}

#[test]
fn codex_removes_stale_claude_runtime_artifacts_before_launch() {
    let (_home, workspace) = temporary_workspace();
    let capabilities = workspace.paths().capabilities_dir();
    std::fs::create_dir_all(&capabilities).expect("capability directory");
    let claude_config = workspace.paths().capability_mcp_config();
    let stale_temp = capabilities.join(".claude-mcp-abandoned.tmp");
    std::fs::write(&claude_config, "stale-secret").expect("stale Claude config");
    std::fs::write(&stale_temp, "stale-secret").expect("stale Claude temp");
    let plan = capability_plan(
        &Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_mcps: Vec::new(),
            allowed_skills: Vec::new(),
            ..Config::default()
        },
        &MachineCapabilityEnvironment::from_value(family_id(), serde_json::json!({}))
            .expect("machine capability environment"),
    )
    .expect("capability plan");
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(plan);

    launch_spec(AgentKind::Codex, "codex", &request).expect("Codex launch spec");

    assert!(!claude_config.exists());
    assert!(!stale_temp.exists());
}

#[test]
fn opencode_removes_stale_frontend_runtime_artifacts_before_launch() {
    let (_home, workspace) = temporary_workspace();
    let capabilities = workspace.paths().capabilities_dir();
    let claude_config = workspace.paths().capability_mcp_config();
    let stale_temp = capabilities.join(".claude-mcp-abandoned.tmp");
    let stale_wrapper = capabilities.join("codex-mcp/stale.sh");
    std::fs::create_dir_all(stale_wrapper.parent().expect("wrapper parent"))
        .expect("capability directories");
    std::fs::write(&claude_config, "stale-secret").expect("stale Claude config");
    std::fs::write(&stale_temp, "stale-secret").expect("stale Claude temp");
    std::fs::write(&stale_wrapper, "stale wrapper").expect("stale Codex wrapper");
    let plan = capability_plan(
        &Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_mcps: Vec::new(),
            allowed_skills: Vec::new(),
            ..Config::default()
        },
        &MachineCapabilityEnvironment::from_value(family_id(), serde_json::json!({}))
            .expect("machine capability environment"),
    )
    .expect("capability plan");
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(plan);

    launch_spec(AgentKind::OpenCode, "opencode", &request).expect("OpenCode launch spec");

    assert!(!claude_config.exists());
    assert!(!stale_temp.exists());
    assert!(!stale_wrapper.exists());
    assert!(!capabilities.join("codex-mcp").exists());
}

#[test]
fn opencode_launch_rollback_removes_rendered_capability_artifacts() {
    let (_home, workspace) = temporary_workspace();
    let launch_actor = actor();
    let plan = capability_plan(
        &Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_mcps: Vec::new(),
            allowed_skills: vec!["todo".to_owned()],
            ..Config::default()
        },
        &MachineCapabilityEnvironment::from_value(family_id(), serde_json::json!({}))
            .expect("machine capability environment"),
    )
    .expect("capability plan");
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        launch_actor,
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(plan);
    let error = launch_with_spawn_failure(AgentKind::OpenCode, "opencode", &request)
        .expect_err("OpenCode spawn must fail");

    assert!(!workspace.paths().capabilities_dir().exists());
    assert_eq!(
        error,
        brain::agent::AgentError::Transport("injected spawn failure".to_owned())
    );
}

#[test]
fn unrestricted_launches_remove_all_workspace_capability_artifacts() {
    let (_home, workspace) = temporary_workspace();
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::Unrestricted,
    );
    for (kind, command) in [
        (AgentKind::Claude, "claude"),
        (AgentKind::Codex, "codex"),
        (AgentKind::OpenCode, "opencode"),
    ] {
        let stale = workspace
            .paths()
            .capability_skills_dir(actor().user_id())
            .join("secret/SKILL.md");
        std::fs::create_dir_all(stale.parent().expect("stale skill parent"))
            .expect("stale skill directory");
        std::fs::write(&stale, "stale-secret").expect("stale skill");

        launch_spec(kind, command, &request).expect("unrestricted launch spec");

        assert!(!workspace.paths().capabilities_dir().exists());
    }
}

#[cfg(unix)]
#[test]
fn unrestricted_cleanup_unlinks_a_capability_symlink_without_touching_its_target() {
    let (_home, workspace) = temporary_workspace();
    let outside = tempfile::tempdir().expect("outside directory");
    let sentinel = outside.path().join("sentinel");
    std::fs::write(&sentinel, "keep").expect("outside sentinel");
    let capabilities = workspace.paths().capabilities_dir();
    std::fs::create_dir_all(capabilities.parent().expect("cache parent")).expect("cache directory");
    std::os::unix::fs::symlink(outside.path(), &capabilities).expect("capability symlink");
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::Unrestricted,
    );

    launch_spec(AgentKind::Codex, "codex", &request).expect("unrestricted launch spec");

    assert!(sentinel.is_file());
    assert!(std::fs::symlink_metadata(&capabilities).is_err());
}

#[cfg(unix)]
#[test]
fn unrestricted_cleanup_rejects_a_symlinked_workspace_cache_root() {
    let (_home, workspace) = temporary_workspace();
    let outside = tempfile::tempdir().expect("outside directory");
    let external_capabilities = outside.path().join("capabilities");
    std::fs::create_dir_all(&external_capabilities).expect("external capability directory");
    let sentinel = external_capabilities.join("sentinel");
    std::fs::write(&sentinel, "keep").expect("outside sentinel");
    let cache_root = workspace.paths().cache_dir();
    std::fs::create_dir_all(cache_root.parent().expect("cache parent"))
        .expect("workspace cache parent");
    std::os::unix::fs::symlink(outside.path(), cache_root).expect("workspace cache symlink");
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::Unrestricted,
    );

    let result = launch_spec(AgentKind::Codex, "codex", &request);

    assert!(
        sentinel.is_file(),
        "cleanup followed the cache-root symlink"
    );
    assert!(result.is_err(), "symlinked cache root must fail closed");
}

#[cfg(unix)]
#[test]
fn skill_cleanup_rejects_a_symlinked_actor_ancestor() {
    let (_home, workspace) = temporary_workspace();
    let outside = tempfile::tempdir().expect("outside directory");
    let external_skills = outside
        .path()
        .join(actor().user_id().as_str())
        .join("skills");
    std::fs::create_dir_all(&external_skills).expect("external skill directory");
    let sentinel = external_skills.join("sentinel");
    std::fs::write(&sentinel, "keep").expect("outside sentinel");
    let capabilities = workspace.paths().capabilities_dir();
    std::fs::create_dir_all(&capabilities).expect("capability directory");
    std::os::unix::fs::symlink(outside.path(), capabilities.join("actors"))
        .expect("actor ancestor symlink");
    let plan = capability_plan(
        &Config {
            access_mode: AccessMode::WorkspaceOnly,
            allowed_mcps: Vec::new(),
            allowed_skills: vec!["todo".to_owned()],
            ..Config::default()
        },
        &MachineCapabilityEnvironment::from_value(family_id(), serde_json::json!({}))
            .expect("machine capability environment"),
    )
    .expect("capability plan");
    let request = LaunchRequest::from_trusted_context(
        Arc::clone(&workspace),
        actor(),
        SessionPlan::fresh(AgentSession::new("session-1").expect("session")),
        None,
        AccessMode::WorkspaceOnly,
    )
    .with_capability_plan(plan);

    let result = launch_spec(AgentKind::Claude, "claude", &request);

    assert!(
        sentinel.is_file(),
        "skill cleanup followed an ancestor symlink"
    );
    assert!(result.is_err(), "symlinked actor ancestor must fail closed");
}
