//! `brain tasks doctor` validates the DB + hook installation. Integration
//! tests exercise the real public API.

use std::path::PathBuf;

use brain::tasks::doctor::{Diagnosis, run_doctor};

fn make_db(path: &std::path::Path) {
    brain::state::Db::open(path).expect("open");
}

#[test]
fn doctor_reports_db_missing_when_path_does_not_exist() {
    let tmp = tempfile::TempDir::new().unwrap();
    let missing: PathBuf = tmp.path().join("nope.db");
    let settings_dir = tmp.path().join("brain").join(".claude");
    let diag = run_doctor(&missing, &settings_dir);
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
    let diag = run_doctor(&db_path, &settings_dir);
    assert!(diag.db_present);
    assert!(diag.db_schema_ok);
}

#[test]
fn doctor_reports_hook_missing_when_settings_file_absent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain").join(".claude");
    let diag = run_doctor(&db_path, &settings_dir);
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
    let diag = run_doctor(&db_path, &settings_dir);
    assert!(!diag.hook_installed);
}

#[test]
fn doctor_reports_hook_installed_when_session_start_entry_references_script() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    make_db(&db_path);
    let settings_dir = tmp.path().join("brain").join(".claude");
    std::fs::create_dir_all(&settings_dir).unwrap();
    // The doctor checks for a SessionStart hook whose command ends in
    // brain/scripts/claude_session_start_hook.py. The absolute prefix need not
    // match — the user may have installed from a different working directory.
    let json = r#"{
      "hooks": {
        "SessionStart": [{
          "hooks": [{"type": "command", "command": "/home/me/scripts/rc/brain/scripts/claude_session_start_hook.py"}]
        }]
      }
    }"#;
    std::fs::write(settings_dir.join("settings.json"), json).unwrap();
    let diag = run_doctor(&db_path, &settings_dir);
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
    let diag = run_doctor(&db_path, &settings_dir);
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
      {"type":"command","command":"/x/rc/brain/scripts/claude_session_start_hook.py"}
    ]}]}}"#;
    std::fs::write(settings_dir.join("settings.json"), json).unwrap();
    let diag = run_doctor(&db_path, &settings_dir);
    assert!(diag.is_ok());
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
