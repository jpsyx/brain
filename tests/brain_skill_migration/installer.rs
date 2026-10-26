use super::*;
use sha2::{Digest as _, Sha256};

pub(super) fn refresh(f: &Fixture, target: &Path, version: &str, phase: &str) -> Output {
    let digest = format!("{:x}", Sha256::digest(std::fs::read(target).unwrap()));
    Command::new(env!("CARGO_BIN_EXE_brain"))
        .args(["__refresh-workspace-skills", "--target-binary"])
        .arg(target)
        .args([
            "--target-version",
            version,
            "--phase",
            phase,
            "--expected-target-sha256",
            &digest,
        ])
        .env_clear()
        .env("HOME", &f.home)
        .env("XDG_CONFIG_HOME", f.home.join(".config"))
        .env("TMPDIR", f.temp.path())
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap()
}

pub(super) fn compatible(f: &Fixture) {
    let path = f.root.join(".config/workspace.json");
    let mut manifest = read_json(&path);
    manifest["minimum_brain_version"] = "0.101.0".into();
    write_json(&path, &manifest);
}

pub(super) fn target(f: &Fixture, version: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = f.temp.path().join("target");
    let script =
        include_str!("../task_deletion_skill_refresh/target.py").replace("0.101.0", version);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn canonical_preflight_uses_distinct_projected_target_sources_before_live_edits() {
    let f = Fixture::new(env!("CARGO_PKG_VERSION"));
    compatible(&f);
    let directory = f.root.join(".config/extensions");
    std::fs::create_dir_all(&directory).unwrap();
    let canonical = b"[agenda:anchors]\nCANONICAL ANCHOR\n";
    std::fs::write(directory.join("agenda.md"), canonical).unwrap();
    let executable = target(&f, "0.101.0");
    let script = std::fs::read_to_string(&executable).unwrap().replace(
        "if ext.exists(): body =",
        "if (root / '.config/extensions/agenda.md').exists():\n    assert ext.exists(), 'missing legacy projection'\n    assert '[todo:anchors]' in ext.read_text()\nif ext.exists(): body =",
    );
    std::fs::write(&executable, script).unwrap();
    let output = refresh(&f, &executable, "0.101.0", "preflight");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!directory.join("todo.md").exists());
    assert_eq!(
        std::fs::read(directory.join("agenda.md")).unwrap(),
        canonical
    );
    assert!(!f.root.join(".agents").exists());
    let output = refresh(&f, &executable, "0.101.0", "apply");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        std::fs::read_to_string(directory.join("todo.md"))
            .unwrap()
            .contains("[todo:anchors]")
    );
    assert_eq!(
        std::fs::read(directory.join("agenda.md")).unwrap(),
        canonical
    );
    assert!(
        std::fs::read_to_string(f.root.join(".agents/skills/todo/SKILL.md"))
            .unwrap()
            .contains("CANONICAL ANCHOR")
    );
}

#[test]
fn audited_105_profile_exports_revision_one_and_schema_one() {
    let f = Fixture::new(env!("CARGO_PKG_VERSION"));
    compatible(&f);
    let executable = target(&f, "0.105.3");
    let output = refresh(&f, &executable, "0.105.3", "apply");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = read_json(&f.cache().join("skills-install/managed.json"));
    assert_eq!(manifest["compatibility_revision"], 1);
    assert_eq!(manifest["schema_version"], 1);
}

#[test]
fn current_target_publishes_complete_instruction_and_extension_package() {
    let f = Fixture::new(env!("CARGO_PKG_VERSION"));
    let output = refresh(
        &f,
        Path::new(env!("CARGO_BIN_EXE_brain")),
        env!("CARGO_PKG_VERSION"),
        "apply",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = read_json(&f.cache().join("skills-install/managed.json"));
    assert!(manifest["package"]["instructions"].is_object());
    assert!(manifest["package"]["extension_snapshot"].is_object());
    assert!(f.root.join("AGENTS.md").is_file());
}

#[test]
fn downgrade_requires_every_root_before_any_target_publication() {
    let f = Fixture::new(env!("CARGO_PKG_VERSION"));
    compatible(&f);
    let registry_path = f.home.join(".config/brain/env.json");
    let mut registry = read_json(&registry_path);
    registry["workspaces"]["missing"] = serde_json::json!({"workspace_id":"22222222-2222-4222-8222-222222222222", "root":f.temp.path().join("missing"), "local_user_id":"member", "env":{}});
    write_json(&registry_path, &registry);
    let executable = target(&f, "0.101.0");
    for phase in ["preflight", "apply"] {
        let output = refresh(&f, &executable, "0.101.0", phase);
        assert!(
            !output.status.success(),
            "unavailable downgrade root accepted"
        );
        assert!(!f.root.join(".agents").exists());
    }
    let output = refresh(
        &f,
        Path::new(env!("CARGO_BIN_EXE_brain")),
        env!("CARGO_PKG_VERSION"),
        "apply",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(f.root.join("AGENTS.md").is_file());
}

#[test]
fn installer_refuses_pending_receive_before_recovery_or_publication() {
    for phase in ["receiving", "reconcile"] {
        let f = Fixture::new(env!("CARGO_PKG_VERSION"));
        compatible(&f);
        f.startup();
        let manifest = f.cache().join("skills-install/managed.json");
        let before = std::fs::read(&manifest).unwrap();
        super::sync_arrival::receiving(&f, phase);
        let executable = target(&f, "0.101.0");
        for phase in ["preflight", "apply"] {
            let output = refresh(&f, &executable, "0.101.0", phase);
            assert!(!output.status.success(), "pending receive accepted");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("receive_pending"),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(std::fs::read(&manifest).unwrap(), before);
        }
    }
}

#[test]
fn current_replica_replans_owned_instructions_without_rebinding_live_archives() {
    let f = Fixture::new("0.105.3");
    compatible(&f);
    f.startup();
    let directory = f.root.join(".config/extensions");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("workspace.md"), "NEW WORKSPACE CONTEXT\n").unwrap();
    let before = std::fs::read(f.root.join("AGENTS.md")).unwrap();
    let target = Path::new(env!("CARGO_BIN_EXE_brain"));
    let output = refresh(&f, target, env!("CARGO_PKG_VERSION"), "preflight");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(f.root.join("AGENTS.md")).unwrap(), before);
    let output = refresh(&f, target, env!("CARGO_PKG_VERSION"), "apply");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        std::fs::read_to_string(f.root.join("AGENTS.md"))
            .unwrap()
            .contains("NEW WORKSPACE CONTEXT")
    );
}

#[test]
fn installer_from_version_projects_scope_before_live_migration_evidence_exists() {
    let f = Fixture::new(env!("CARGO_PKG_VERSION"));
    compatible(&f);
    let config = f.root.join(".config/config.json");
    write_json(&config, &json!({"allowed_skills":["todo"]}));
    let executable = Path::new(env!("CARGO_BIN_EXE_brain"));
    let digest = format!("{:x}", Sha256::digest(std::fs::read(executable).unwrap()));
    for phase in ["preflight", "apply"] {
        let output = Command::new(executable)
            .args(["__refresh-workspace-skills", "--target-binary"])
            .arg(executable)
            .args([
                "--target-version",
                env!("CARGO_PKG_VERSION"),
                "--phase",
                phase,
                "--expected-target-sha256",
                &digest,
                "--from-version",
                "0.105.3",
            ])
            .env_clear()
            .env("HOME", &f.home)
            .env("XDG_CONFIG_HOME", f.home.join(".config"))
            .env("TMPDIR", f.temp.path())
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            read_json(&config)["allowed_skills"],
            if phase == "preflight" {
                json!(["todo"])
            } else {
                json!(["todo", "agenda"])
            }
        );
    }
}

#[test]
fn changed_instruction_and_config_beforeimages_refuse_candidate_publication() {
    for relative in ["AGENTS.md", ".config/config.json"] {
        let f = Fixture::new(env!("CARGO_PKG_VERSION"));
        compatible(&f);
        std::fs::write(f.root.join("AGENTS.md"), "USER DOCUMENT\n").unwrap();
        write_json(
            &f.root.join(".config/config.json"),
            &json!({"skills_auto_sync":false}),
        );
        let directory = f.root.join(".config/extensions");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("todo.md"), "[todo:anchors]\nANCHOR\n").unwrap();
        let executable = target(&f, "0.101.0");
        let live_path = serde_json::to_string(&f.root.join(relative)).unwrap();
        let replacement = if relative == "AGENTS.md" {
            "CHANGED USER DOCUMENT\n"
        } else {
            "{\"skills_auto_sync\":false,\"agenda_dir\":\"changed\"}"
        };
        let script = std::fs::read_to_string(&executable).unwrap();
        std::fs::write(&executable, script.replace("if ext.exists(): body =", &format!("if ext.exists(): pathlib.Path({live_path}).write_text({})\nif ext.exists(): body =", serde_json::to_string(replacement).unwrap()))).unwrap();
        let output = refresh(&f, &executable, "0.101.0", "apply");
        assert!(!output.status.success(), "{relative}");
        assert_eq!(
            std::fs::read_to_string(f.root.join(relative)).unwrap(),
            replacement
        );
        assert!(!f.root.join(".agents").exists());
    }
}

#[test]
fn current_target_instruction_output_must_match_the_prepared_package() {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new(env!("CARGO_PKG_VERSION"));
    let executable = f.temp.path().join("current-wrapper");
    let current = serde_json::to_string(env!("CARGO_BIN_EXE_brain")).unwrap();
    std::fs::write(&executable, format!("#!/usr/bin/python3\nimport pathlib, subprocess, sys\nresult = subprocess.run([{current}] + sys.argv[1:])\nif sys.argv[-2:] == ['skills', 'sync'] and result.returncode == 0:\n    pathlib.Path('AGENTS.md').write_text('unexpected target document')\nsys.exit(result.returncode)\n")).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = refresh(&f, &executable, env!("CARGO_PKG_VERSION"), "preflight");
    assert!(
        !output.status.success(),
        "unvalidated instructions accepted"
    );
    assert!(!f.root.join("AGENTS.md").exists());
}

#[test]
fn audited_105_profile_refuses_a_future_task_schema() {
    let f = Fixture::new(env!("CARGO_PKG_VERSION"));
    compatible(&f);
    std::fs::create_dir_all(f.root.join("tasks")).unwrap();
    std::fs::write(
        f.root.join("tasks/SCHEMA.json"),
        br#"{"task_schema_version":3}"#,
    )
    .unwrap();
    let executable = target(&f, "0.105.3");
    let output = refresh(&f, &executable, "0.105.3", "preflight");
    assert!(!output.status.success(), "future task schema accepted");
    assert!(!f.root.join(".agents").exists());
}
