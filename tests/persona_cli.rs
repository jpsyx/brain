//! End-to-end persona behavior: per-user reads and writes, legacy migration,
//! and the missing-persona nudge every command performs.

use std::process::Command;

use serde_json::json;
use tempfile::TempDir;

fn brain_command(home: &TempDir, config_home: &TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_brain"));
    command
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("NO_COLOR", "1");
    command
}

fn run(home: &TempDir, config_home: &TempDir, args: &[&str]) -> (String, String) {
    let output = brain_command(home, config_home)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("run brain {args:?}: {error}"));
    assert!(
        output.status.success(),
        "brain {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        String::from_utf8(output.stderr).expect("UTF-8 stderr"),
    )
}

/// A registered, ready workspace whose local person is `pablo`.
fn ready_workspace() -> (TempDir, TempDir) {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    let env_dir = config_home.path().join("brain");
    std::fs::create_dir_all(&env_dir).expect("env dir");
    std::fs::write(
        env_dir.join("env.json"),
        serde_json::to_string_pretty(&json!({"root": "~/brain"})).expect("serialize env"),
    )
    .expect("write env");
    let output = brain_command(&home, &config_home)
        .args([
            "workspace",
            "repair",
            "--manifest",
            "--local-user-id",
            "pablo",
        ])
        .output()
        .expect("repair workspace");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    run(
        &home,
        &config_home,
        &["user", "add", "--id", "pablo", "--name", "Pablo"],
    );
    (home, config_home)
}

fn personalization_path(home: &TempDir) -> std::path::PathBuf {
    home.path().join("brain/.config/personalization.json")
}

#[test]
fn a_persona_is_written_read_and_listed_under_its_own_user_id() {
    let (home, config_home) = ready_workspace();
    run(
        &home,
        &config_home,
        &["user", "add", "--id", "sam", "--name", "Sam"],
    );

    run(&home, &config_home, &["persona", "set", "role=CEO"]);
    run(
        &home,
        &config_home,
        &["persona", "set", "role=designer", "--user", "sam"],
    );

    let (mine, _) = run(&home, &config_home, &["persona", "show"]);
    assert!(mine.contains("user: pablo (this machine)"), "{mine}");
    assert!(mine.contains("role: CEO"), "{mine}");

    // One member's write must not disturb another's entry.
    let (theirs, _) = run(&home, &config_home, &["persona", "get", "sam"]);
    assert!(theirs.contains("user: sam"), "{theirs}");
    assert!(!theirs.contains("this machine"), "{theirs}");
    assert!(theirs.contains("role: designer"), "{theirs}");

    let (one_field, _) = run(&home, &config_home, &["persona", "get", "sam", "role"]);
    assert_eq!(one_field.trim(), "designer");

    let (everyone, _) = run(&home, &config_home, &["persona", "list"]);
    assert!(
        everyone.contains("user: pablo (this machine)"),
        "{everyone}"
    );
    assert!(everyone.contains("user: sam"), "{everyone}");
    assert!(everyone.contains("role: CEO"), "{everyone}");
    assert!(everyone.contains("role: designer"), "{everyone}");
}

#[test]
fn a_member_of_the_workspace_appears_in_the_list_before_they_personalize() {
    let (home, config_home) = ready_workspace();
    run(
        &home,
        &config_home,
        &["user", "add", "--id", "sam", "--name", "Sam"],
    );
    run(&home, &config_home, &["persona", "set", "role=CEO"]);

    let (everyone, _) = run(&home, &config_home, &["persona", "list"]);

    // A skill must learn that Sam exists even with nothing filled in.
    let sam = everyone
        .split("\n\n")
        .find(|block| block.starts_with("user: sam"))
        .unwrap_or_else(|| panic!("no block for sam:\n{everyone}"));
    assert!(sam.contains("role: (unset)"), "{sam}");
}

#[test]
fn setting_a_persona_for_an_unknown_user_is_rejected_by_name() {
    let (home, config_home) = ready_workspace();
    run(&home, &config_home, &["persona", "set", "role=CEO"]);

    let output = brain_command(&home, &config_home)
        .args(["persona", "set", "role=CEO", "--user", "ghost"])
        .output()
        .expect("run brain persona set");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(stderr.contains("unknown user `ghost`"), "{stderr}");
}

#[test]
fn a_legacy_single_persona_file_is_read_and_rewritten_under_the_local_user() {
    let (home, config_home) = ready_workspace();
    std::fs::write(
        personalization_path(&home),
        br#"{"name": "Pablo", "role": "CEO", "works_for": "Avandar"}"#,
    )
    .expect("write legacy personalization");

    // Read migrates in memory, so the value is visible with no migration step.
    let (shown, _) = run(&home, &config_home, &["persona", "show"]);
    assert!(shown.contains("user: pablo (this machine)"), "{shown}");
    assert!(shown.contains("works_for: Avandar"), "{shown}");

    // The next write persists the keyed schema without losing the old values.
    run(&home, &config_home, &["persona", "set", "role=founder"]);
    let stored = std::fs::read_to_string(personalization_path(&home)).expect("read store");
    let stored: serde_json::Value = serde_json::from_str(&stored).expect("stored JSON");
    assert_eq!(stored["schema_version"], json!(2));
    assert_eq!(stored["personas"]["pablo"]["role"], json!("founder"));
    assert_eq!(stored["personas"]["pablo"]["works_for"], json!("Avandar"));
}

#[test]
fn a_headless_command_reports_a_missing_persona_without_failing() {
    let (home, config_home) = ready_workspace();

    // `output()` gives the child a piped stdin, so there is no terminal to
    // prompt on: brain must say what to run and get on with the command.
    let (stdout, stderr) = run(&home, &config_home, &["config", "get", "day_rollover_hour"]);

    assert!(stderr.contains("pablo has no persona yet"), "{stderr}");
    assert!(stderr.contains("brain persona set role=<ROLE>"), "{stderr}");
    assert_eq!(stdout.trim(), "6", "the command itself still ran");
}

#[test]
fn a_personalized_user_is_never_nudged_again() {
    let (home, config_home) = ready_workspace();
    run(&home, &config_home, &["persona", "set", "role=CEO"]);

    let (_, stderr) = run(&home, &config_home, &["config", "get", "day_rollover_hour"]);

    assert!(!stderr.contains("has no persona"), "{stderr}");
}

#[test]
fn the_persona_command_itself_does_not_nudge_before_collecting() {
    let (home, config_home) = ready_workspace();

    let (_, stderr) = run(&home, &config_home, &["persona", "list"]);

    assert!(!stderr.contains("has no persona"), "{stderr}");
}
