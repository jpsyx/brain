//! `brain tasks doctor` validates the DB + hook installation. Integration
//! tests exercise the real public API.

use std::path::PathBuf;

use brain::tasks::doctor::{Diagnosis, run_doctor};

fn make_db(path: &std::path::Path) {
    brain::state::Db::open_path(path).expect("open");
}

#[test]
fn doctor_reports_db_missing_when_path_does_not_exist() {
    let tmp = tempfile::TempDir::new().unwrap();
    let missing: PathBuf = tmp.path().join("nope.db");
    let settings_dir = tmp.path().join("brain").join(".claude");
    let diag = run_doctor(&missing, &settings_dir, false);
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
    let diag = run_doctor(&db_path, &settings_dir, false);
    assert!(diag.db_present);
    assert!(diag.db_schema_ok);
}

#[test]
fn doctor_reports_hook_missing_when_settings_file_absent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain").join(".claude");
    let diag = run_doctor(&db_path, &settings_dir, false);
    assert!(!diag.hook_installed, "no settings file => no hook");
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
    let diag = run_doctor(&db_path, &settings_dir, false);
    assert!(!diag.hook_installed);
}

#[test]
fn doctor_reports_hook_installed_when_session_start_entry_references_script() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain").join(".claude");
    std::fs::create_dir_all(&settings_dir).unwrap();
    // Both frontends use the project-relative script installed by Brain.
    let json = r#"{
      "hooks": {
        "SessionStart": [{
          "hooks": [{"type": "command", "command": "python3 .claude/brain-hooks/claude_session_start_hook.py"}]
        }]
      }
    }"#;
    std::fs::write(settings_dir.join("settings.json"), json).unwrap();
    let diag = run_doctor(&db_path, &settings_dir, false);
    assert!(diag.hook_installed, "diag={diag:?}");
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
    let diag = run_doctor(&db_path, &settings_dir, false);
    assert!(!diag.hook_installed, "a legacy tasks-path hook is not ours");
}

#[test]
fn diagnosis_is_ok_when_all_checks_pass() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain").join(".claude");
    std::fs::create_dir_all(&settings_dir).unwrap();
    let json = r#"{"hooks":{"SessionStart":[{"hooks":[
      {"type":"command","command":"python3 .claude/brain-hooks/claude_session_start_hook.py"}
    ]}]}}"#;
    std::fs::write(settings_dir.join("settings.json"), json).unwrap();
    let codex_hooks = tmp.path().join(".codex/hooks.json");
    std::fs::create_dir_all(codex_hooks.parent().unwrap()).unwrap();
    std::fs::write(
        &codex_hooks,
        r#"{"hooks":{"SessionStart":[{"hooks":[
          {"type":"command","command":"python3 .claude/brain-hooks/claude_session_start_hook.py"}
        ]}]}}"#,
    )
    .unwrap();
    let plugin = tmp.path().join("brain/.opencode/plugins/brain.js");
    std::fs::create_dir_all(plugin.parent().unwrap()).unwrap();
    std::fs::write(plugin, "session.created session.idle").unwrap();
    let diag = brain::tasks::doctor::run_doctor_with_frontends(
        &db_path,
        &settings_dir,
        &codex_hooks,
        false,
    );
    assert!(diag.claude_hook_installed);
    assert!(diag.codex_hook_installed);
    assert!(diag.is_ok());
}

#[test]
fn doctor_requires_all_functional_frontend_integrations() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain/.claude");
    std::fs::create_dir_all(&settings_dir).unwrap();
    std::fs::write(
        settings_dir.join("settings.json"),
        r#"{"hooks":{"SessionStart":[{"hooks":[
          {"type":"command","command":"python3 .claude/brain-hooks/claude_session_start_hook.py"}
        ]}]}}"#,
    )
    .unwrap();
    let codex_hooks = tmp.path().join(".codex/hooks.json");

    let diag = brain::tasks::doctor::run_doctor_with_frontends(
        &db_path,
        &settings_dir,
        &codex_hooks,
        false,
    );

    assert!(diag.claude_hook_installed);
    assert!(!diag.codex_hook_installed);
    assert!(!diag.opencode_plugin_installed);
    assert!(!diag.is_ok());
}

#[test]
fn diagnosis_is_not_ok_when_anything_failed() {
    let d = Diagnosis {
        db_present: true,
        db_schema_ok: true,
        hook_installed: false,
        ..Diagnosis::default()
    };
    assert!(!d.is_ok());
}
