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

fn write_env(config_home: &TempDir) -> std::path::PathBuf {
    let env_dir = config_home.path().join("brain");
    std::fs::create_dir_all(&env_dir).expect("env dir");
    let path = env_dir.join("env.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "root": "~/brain",
            "sync": {
                "enabled": true,
                "remote": {"bucket": "pablo-brain", "credentials": {"key_id": "abc"}}
            }
        }))
        .expect("serialize env"),
    )
    .expect("write env");
    path
}

fn make_ready(home: &TempDir, config_home: &TempDir) {
    let output = brain_command(home, config_home)
        .args([
            "workspace",
            "repair",
            "--manifest",
            "--local-user-id",
            "test-user",
        ])
        .output()
        .expect("repair migrated workspace");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn env_list_get_and_set_support_recursive_dotted_paths() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    let env_path = write_env(&config_home);
    make_ready(&home, &config_home);

    let listed = brain_command(&home, &config_home)
        .args(["env", "list"])
        .output()
        .expect("env list");
    assert!(listed.status.success());
    let stdout = String::from_utf8_lossy(&listed.stdout);
    assert!(
        stdout.contains("sync.remote.credentials.key_id"),
        "{stdout}"
    );
    assert!(stdout.contains("sync.enabled"), "{stdout}");

    let got = brain_command(&home, &config_home)
        .args(["env", "get", "sync.remote.credentials.key_id"])
        .output()
        .expect("env get");
    assert!(got.status.success());
    assert_eq!(String::from_utf8_lossy(&got.stdout).trim(), "abc");

    let set = brain_command(&home, &config_home)
        .args(["env", "set", "sync.remote.credentials.key_id=updated"])
        .output()
        .expect("env set");
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );

    let saved: brain::workspace::MachineRegistry =
        serde_json::from_str(&std::fs::read_to_string(env_path).expect("read env"))
            .expect("parse registry");
    let env = &saved.select(None).expect("default workspace").record().env;
    assert_eq!(env["sync"]["enabled"], true);
    assert_eq!(env["sync"]["remote"]["bucket"], "pablo-brain");
    assert_eq!(env["sync"]["remote"]["credentials"]["key_id"], "updated");
}

/// Attach a second workspace so the breakdown has more than one block to show.
fn attach_second_workspace(home: &TempDir, config_home: &TempDir, name: &str) {
    let root = home.path().join(name);
    let output = brain_command(home, config_home)
        .args([
            "workspace",
            "create",
            "--name",
            name,
            "--root",
            &root.display().to_string(),
        ])
        .output()
        .expect("create second workspace");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let repaired = brain_command(home, config_home)
        .args([
            "-w",
            name,
            "workspace",
            "repair",
            "--manifest",
            "--local-user-id",
            "test-user",
        ])
        .output()
        .expect("repair second workspace");
    assert!(
        repaired.status.success(),
        "{}",
        String::from_utf8_lossy(&repaired.stderr)
    );
}

#[test]
fn bare_env_breaks_down_machine_global_values_and_every_workspace() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    write_env(&config_home);
    make_ready(&home, &config_home);
    attach_second_workspace(&home, &config_home, "family");

    for args in [vec!["env"], vec!["env", "list"]] {
        let output = brain_command(&home, &config_home)
            .args(&args)
            .output()
            .expect("env breakdown");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Machine-global values (everything outside the `workspaces` key).
        assert!(stdout.contains("Global"), "{args:?}: {stdout}");
        assert!(stdout.contains("schema_version"), "{args:?}: {stdout}");
        assert!(stdout.contains("default_workspace"), "{args:?}: {stdout}");
        // One block per registered workspace, not only the selected one.
        assert!(stdout.contains("Workspaces"), "{args:?}: {stdout}");
        assert!(stdout.contains("family"), "{args:?}: {stdout}");
        // Nested per-workspace paths still list.
        assert!(stdout.contains("sync.enabled"), "{args:?}: {stdout}");
        assert!(
            stdout.contains("sync.remote.credentials.key_id"),
            "{args:?}: {stdout}"
        );
        // And the legend still explains what each name means.
        assert!(stdout.contains("Variables"), "{args:?}: {stdout}");
    }
}

#[test]
fn the_breakdown_redacts_a_non_selected_workspaces_secret() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    write_env(&config_home);
    make_ready(&home, &config_home);
    attach_second_workspace(&home, &config_home, "family");
    let secret = "whsec_other-workspace-must-stay-private";
    let stored = brain_command(&home, &config_home)
        .args([
            "-w",
            "family",
            "env",
            "set",
            &format!("resend_webhook_signing_secret={secret}"),
        ])
        .output()
        .expect("set secret on the non-selected workspace");
    assert!(
        stored.status.success(),
        "{}",
        String::from_utf8_lossy(&stored.stderr)
    );

    let output = brain_command(&home, &config_home)
        .arg("env")
        .output()
        .expect("env breakdown");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains(secret), "{stdout}");
    assert!(stdout.contains("(set)"), "{stdout}");
}

#[test]
fn sensitive_env_assignment_never_echoes_the_raw_value() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    write_env(&config_home);
    make_ready(&home, &config_home);
    let secret = "whsec_assignment-must-stay-private";

    let output = brain_command(&home, &config_home)
        .args([
            "env",
            "set",
            &format!("resend_webhook_signing_secret={secret}"),
        ])
        .output()
        .expect("sensitive env set");

    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
    assert!(String::from_utf8_lossy(&output.stdout).contains("saved"));
}

#[test]
fn the_default_agent_frontend_is_claude_until_this_machine_says_otherwise() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    write_env(&config_home);
    make_ready(&home, &config_home);

    let unset = brain_command(&home, &config_home)
        .args(["env", "get", "default_agent_frontend"])
        .output()
        .expect("read the unset default");
    assert_eq!(
        String::from_utf8_lossy(&unset.stdout).trim(),
        "claude",
        "{}",
        String::from_utf8_lossy(&unset.stderr)
    );

    // The flag spells it `--open-code`; the store keeps one canonical spelling.
    let set = brain_command(&home, &config_home)
        .args(["env", "set", "default_agent_frontend=Open-Code"])
        .output()
        .expect("set the default frontend");
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    // The confirmation must report what was stored, not what was typed.
    let confirmation = String::from_utf8_lossy(&set.stdout);
    assert!(confirmation.contains("opencode"), "{confirmation}");
    assert!(!confirmation.contains("Open-Code"), "{confirmation}");
    let stored = brain_command(&home, &config_home)
        .args(["env", "get", "default_agent_frontend"])
        .output()
        .expect("read the stored default");
    assert_eq!(String::from_utf8_lossy(&stored.stdout).trim(), "opencode");
}

#[test]
fn an_unknown_default_agent_frontend_is_rejected_with_the_valid_set() {
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    write_env(&config_home);
    make_ready(&home, &config_home);

    let output = brain_command(&home, &config_home)
        .args(["env", "set", "default_agent_frontend=gemini"])
        .output()
        .expect("reject an unknown frontend");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("claude, codex, opencode"), "{stderr}");
    // The store keeps the previous (unset → default) value.
    let stored = brain_command(&home, &config_home)
        .args(["env", "get", "default_agent_frontend"])
        .output()
        .expect("read the default after rejection");
    assert_eq!(String::from_utf8_lossy(&stored.stdout).trim(), "claude");
}

#[test]
fn skill_sessions_round_trip_as_an_array_and_per_field_path() {
    // The CLI half of skill sessions: a human (or an agent) must be able to write
    // the whole array and then amend one field, and `brain env` must show it.
    let home = tempfile::tempdir().expect("home tempdir");
    let config_home = tempfile::tempdir().expect("config tempdir");
    let env_path = write_env(&config_home);
    make_ready(&home, &config_home);

    let set = brain_command(&home, &config_home)
        .args([
            "env",
            "set",
            r#"skill_sessions=[{"title":"Email triage","prompt":"/email-triage","command_label":"Run email triage"}]"#,
        ])
        .output()
        .expect("env set skill_sessions");
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );

    let amended = brain_command(&home, &config_home)
        .args(["env", "set", "skill_sessions.0.prompt=/email-triage --fast"])
        .output()
        .expect("env set nested skill session field");
    assert!(
        amended.status.success(),
        "{}",
        String::from_utf8_lossy(&amended.stderr)
    );

    let saved: brain::workspace::MachineRegistry =
        serde_json::from_str(&std::fs::read_to_string(env_path).expect("read env"))
            .expect("parse registry");
    let env = &saved.select(None).expect("default workspace").record().env;
    assert_eq!(env["skill_sessions"][0]["title"], "Email triage");
    assert_eq!(env["skill_sessions"][0]["prompt"], "/email-triage --fast");
    assert_eq!(
        brain::skill_session::available(false, env.get("skill_sessions"))[0].command_label,
        "Run email triage"
    );

    let listed = brain_command(&home, &config_home)
        .args(["env", "list"])
        .output()
        .expect("env list");
    let stdout = String::from_utf8_lossy(&listed.stdout);
    assert!(stdout.contains("skill_sessions"), "{stdout}");
}
