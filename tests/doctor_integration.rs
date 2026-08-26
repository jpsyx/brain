//! `brain tasks doctor` validates the DB + hook installation. Integration
//! tests exercise the real public API.

use std::path::PathBuf;

use brain::session::AgentKind;
use brain::tasks::doctor::{Diagnosis, run_doctor};

fn make_db(path: &std::path::Path) {
    brain::state::Db::open_path(path).expect("open");
}

fn compatible_opencode() -> [(AgentKind, Result<Option<String>, brain::agent::AgentError>); 1] {
    [(AgentKind::OpenCode, Ok(Some("1.18.14".to_owned())))]
}

fn install_bridge_files(settings_dir: &std::path::Path) {
    let hooks = settings_dir
        .parent()
        .expect("workspace root")
        .join(".brain/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::write(
        hooks.join("agent_session_start_hook.py"),
        include_str!("../scripts/agent_session_start_hook.py"),
    )
    .unwrap();
    std::fs::write(
        hooks.join("agent_session_stop_hook.py"),
        include_str!("../scripts/agent_session_stop_hook.py"),
    )
    .unwrap();
    std::fs::write(
        hooks.join("receiver_observation_bridge.py"),
        include_str!("../scripts/receiver_observation_bridge.py"),
    )
    .unwrap();
}

fn hook_settings() -> &'static str {
    r#"{"hooks":{
      "SessionStart":[{"hooks":[
        {"type":"command","command":"python3 \"${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/agent_session_start_hook.py\""}
      ]}],
      "Stop":[{"hooks":[
        {"type":"command","command":"python3 \"${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/agent_session_stop_hook.py\""}
      ]}],
      "UserPromptSubmit":[{"hooks":[
        {"type":"command","command":"python3 \"${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/receiver_observation_bridge.py\""}
      ]}],
      "PostToolUse":[{"hooks":[
        {"type":"command","command":"python3 \"${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/receiver_observation_bridge.py\""}
      ]}]
    }}"#
}

fn codex_hook_settings() -> &'static str {
    r#"{"hooks":{
      "SessionStart":[{"hooks":[
        {"type":"command","command":"python3 \"${BRAIN_ROOT}/.brain/hooks/agent_session_start_hook.py\""}
      ]}],
      "Stop":[{"hooks":[
        {"type":"command","command":"python3 \"${BRAIN_ROOT}/.brain/hooks/agent_session_stop_hook.py\""}
      ]}],
      "UserPromptSubmit":[{"hooks":[
        {"type":"command","command":"python3 \"${BRAIN_ROOT}/.brain/hooks/receiver_observation_bridge.py\""}
      ]}],
      "PostToolUse":[{"hooks":[
        {"type":"command","command":"python3 \"${BRAIN_ROOT}/.brain/hooks/receiver_observation_bridge.py\""}
      ]}]
    }}"#
}

#[test]
fn doctor_reports_db_missing_when_path_does_not_exist() {
    let tmp = tempfile::TempDir::new().unwrap();
    let missing: PathBuf = tmp.path().join("nope.db");
    let settings_dir = tmp.path().join("brain").join(".claude");
    let diag = run_doctor(&missing, &settings_dir, false, &compatible_opencode());
    assert!(!diag.db_present, "db should be reported missing");
    // Schema check is N/A when the file isn't there.
    assert!(diag.db_schema_ok, "schema check is vacuously OK");
}

#[test]
fn doctor_reports_db_schema_ok_when_db_freshly_opened() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain").join(".claude");
    let diag = run_doctor(&db_path, &settings_dir, false, &compatible_opencode());
    assert!(diag.db_present);
    assert!(diag.db_schema_ok);
}

#[test]
fn doctor_reports_hook_missing_when_settings_file_absent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain").join(".claude");
    let diag = run_doctor(&db_path, &settings_dir, false, &compatible_opencode());
    assert!(
        !diag.frontend_ready(AgentKind::Claude),
        "no settings file => no hook"
    );
}

#[test]
fn doctor_reports_hook_missing_when_settings_lacks_session_start_entry() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain").join(".claude");
    std::fs::create_dir_all(&settings_dir).unwrap();
    std::fs::write(
        settings_dir.join("settings.json"),
        r#"{"hooks": {"PreToolUse": []}}"#,
    )
    .unwrap();
    let diag = run_doctor(&db_path, &settings_dir, false, &compatible_opencode());
    assert!(!diag.frontend_ready(AgentKind::Claude));
}

#[test]
fn doctor_reports_hook_installed_when_session_start_entry_references_script() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain").join(".claude");
    std::fs::create_dir_all(&settings_dir).unwrap();
    // Both frontends use the project-relative script installed by Brain.
    std::fs::write(settings_dir.join("settings.json"), hook_settings()).unwrap();
    install_bridge_files(&settings_dir);
    let diag = run_doctor(&db_path, &settings_dir, false, &compatible_opencode());
    assert!(diag.frontend_ready(AgentKind::Claude), "diag={diag:?}");
}

#[test]
fn doctor_requires_the_complete_start_and_stop_hook_pair() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain/.claude");
    std::fs::create_dir_all(&settings_dir).unwrap();
    std::fs::write(
        settings_dir.join("settings.json"),
        r#"{"hooks":{"SessionStart":[{"hooks":[
          {"type":"command","command":"python3 \"${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT}}/.brain/hooks/agent_session_start_hook.py\""}
        ]}]}}"#,
    )
    .unwrap();
    install_bridge_files(&settings_dir);

    let diag = run_doctor(&db_path, &settings_dir, false, &compatible_opencode());

    assert!(!diag.frontend_ready(AgentKind::Claude));
}

#[test]
fn doctor_rejects_stale_opencode_plugin_and_bridge_contents() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain/.claude");
    install_bridge_files(&settings_dir);
    let plugin = tmp.path().join("brain/.opencode/plugins/brain.js");
    std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
    std::fs::write(&plugin, "stale plugin").unwrap();

    let stale_plugin = run_doctor(&db_path, &settings_dir, false, &compatible_opencode());
    assert!(!stale_plugin.frontend_ready(AgentKind::OpenCode));

    std::fs::write(&plugin, include_str!("../scripts/opencode_brain_plugin.js")).unwrap();
    std::fs::write(
        settings_dir
            .parent()
            .unwrap()
            .join(".brain/hooks/agent_session_start_hook.py"),
        "stale bridge",
    )
    .unwrap();
    let stale_bridge = run_doctor(&db_path, &settings_dir, false, &compatible_opencode());
    assert!(!stale_bridge.frontend_ready(AgentKind::OpenCode));

    install_bridge_files(&settings_dir);
    std::fs::write(
        settings_dir
            .parent()
            .unwrap()
            .join(".brain/hooks/receiver_observation_bridge.py"),
        "stale observation bridge",
    )
    .unwrap();
    let stale_observation = run_doctor(&db_path, &settings_dir, false, &compatible_opencode());
    assert!(!stale_observation.frontend_ready(AgentKind::OpenCode));
}

#[test]
fn a_legacy_tasks_hook_alone_does_not_count_as_installed() {
    // The pre-merge standalone `tasks` shell installed an identically-named
    // hook under rc/tasks. After the merge only the brain-path hook counts, so
    // a stray legacy tasks-path hook must NOT register as installed.
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain").join(".claude");
    std::fs::create_dir_all(&settings_dir).unwrap();
    let json = r#"{"hooks":{"SessionStart":[{"hooks":[
      {"type":"command","command":"/home/me/scripts/rc/tasks/scripts/claude_session_start_hook.py"}
    ]}]}}"#;
    std::fs::write(settings_dir.join("settings.json"), json).unwrap();
    let diag = run_doctor(&db_path, &settings_dir, false, &compatible_opencode());
    assert!(
        !diag.frontend_ready(AgentKind::Claude),
        "a legacy tasks-path hook is not ours"
    );
}

#[test]
fn diagnosis_is_ok_when_all_checks_pass() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain").join(".claude");
    std::fs::create_dir_all(&settings_dir).unwrap();
    std::fs::write(settings_dir.join("settings.json"), hook_settings()).unwrap();
    install_bridge_files(&settings_dir);
    let codex_hooks = tmp.path().join("brain/.codex/hooks.json");
    std::fs::create_dir_all(codex_hooks.parent().unwrap()).unwrap();
    std::fs::write(&codex_hooks, codex_hook_settings()).unwrap();
    let plugin = tmp.path().join("brain/.opencode/plugins/brain.js");
    std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
    std::fs::write(plugin, include_str!("../scripts/opencode_brain_plugin.js")).unwrap();
    let wrong_compatibility = [(AgentKind::Claude, Ok(Some("compatible".to_owned())))];
    let missing_opencode = brain::tasks::doctor::run_doctor_with_frontends(
        &db_path,
        &settings_dir,
        &codex_hooks,
        false,
        &wrong_compatibility,
    );
    assert!(!missing_opencode.is_ok());

    let diag = brain::tasks::doctor::run_doctor_with_frontends(
        &db_path,
        &settings_dir,
        &codex_hooks,
        false,
        &compatible_opencode(),
    );
    assert!(diag.frontend_ready(AgentKind::Claude));
    assert!(diag.frontend_ready(AgentKind::Codex));
    assert_eq!(
        diag.frontend_health()
            .iter()
            .map(brain::tasks::doctor::FrontendHealth::kind)
            .collect::<Vec<_>>(),
        brain::agent::AgentKind::ALL
    );
    assert!(
        diag.frontend_health()
            .iter()
            .all(brain::tasks::doctor::FrontendHealth::is_ready)
    );
    assert!(diag.is_ok());
}

#[test]
fn doctor_requires_all_functional_frontend_integrations() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain/.claude");
    std::fs::create_dir_all(&settings_dir).unwrap();
    std::fs::write(settings_dir.join("settings.json"), hook_settings()).unwrap();
    install_bridge_files(&settings_dir);
    let codex_hooks = tmp.path().join("brain/.codex/hooks.json");

    let diag = brain::tasks::doctor::run_doctor_with_frontends(
        &db_path,
        &settings_dir,
        &codex_hooks,
        false,
        &compatible_opencode(),
    );

    assert!(diag.frontend_ready(AgentKind::Claude));
    assert!(!diag.frontend_ready(AgentKind::Codex));
    assert!(!diag.frontend_ready(AgentKind::OpenCode));
    assert!(!diag.is_ok());
}

#[test]
fn diagnosis_is_not_ok_when_anything_failed() {
    let mut d = Diagnosis::default();
    d.db_present = true;
    d.db_schema_ok = true;
    assert!(!d.is_ok());
}
