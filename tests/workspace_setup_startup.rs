//! Setup requests repair strict identity before mandatory managed compatibility.
#[allow(dead_code)]
#[path = "task_deletion_skill_migration/support.rs"]
mod support;
use std::fs;
use support::{Fixture, assert_ok, todo};

fn repair(f: &Fixture) -> std::process::Output {
    f.run(&[
        "workspace",
        "repair",
        "--manifest",
        "--local-user-id",
        "test-user",
        "-w",
        "alpha",
    ])
}
fn stamp(f: &Fixture, version: &str) {
    fs::write(f.root("config/brain/migrations/version"), version).unwrap();
}
fn recorded(f: &Fixture) -> String {
    fs::read_to_string(f.root("config/brain/migrations/version"))
        .unwrap()
        .trim()
        .to_owned()
}

#[test]
fn setup_repairs_missing_identity_before_reached_and_omitted_managed_entry_once() {
    for version in [env!("CARGO_PKG_VERSION"), "0.102.25", "0.101.0"] {
        let mut f = Fixture::new();
        let root = f.add("alpha");
        let manifest = root.join(".config/workspace.json");
        fs::remove_file(&manifest).unwrap();
        stamp(&f, version);
        let output = repair(&f);
        assert_ok(&output);
        let restored: serde_json::Value =
            serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
        assert_eq!(
            restored["workspace_id"],
            f.registry["workspaces"]["alpha"]["workspace_id"]
        );
        assert!(todo(&root).is_file());
        assert_eq!(recorded(&f), env!("CARGO_PKG_VERSION"));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout)
                .matches("Workspace setup repaired")
                .count(),
            1
        );
    }
}

#[test]
fn setup_legacy_repair_create_and_attach_complete_before_compatibility() {
    for action in ["repair", "create", "attach"] {
        let f = Fixture::new();
        let legacy = f.root("legacy");
        fs::create_dir_all(&legacy).unwrap();
        let flat = serde_json::to_vec(&serde_json::json!({"root":legacy})).unwrap();
        fs::write(f.registry_path(), &flat).unwrap();
        let requested = f.root("new-workspace");
        if action == "attach" {
            brain::workspace::WorkspaceManifest::new(brain::workspace::WorkspaceId::new())
                .write_new(&requested)
                .unwrap();
        }
        let original = if action == "attach" {
            Some(fs::read(requested.join(".config/workspace.json")).unwrap())
        } else {
            None
        };
        let output = match action {
            "repair" => f.run(&[
                "workspace",
                "repair",
                "--manifest",
                "--local-user-id",
                "test-user",
            ]),
            "create" => f.run(&[
                "workspace",
                "create",
                "--name",
                "new-workspace",
                "--root",
                requested.to_str().unwrap(),
            ]),
            _ => f.run(&["workspace", "attach", requested.to_str().unwrap()]),
        };
        assert_ok(&output);
        assert_eq!(
            fs::read(f.root("config/brain/env.json.legacy-backup")).unwrap(),
            flat
        );
        let registry: serde_json::Value =
            serde_json::from_slice(&fs::read(f.registry_path()).unwrap()).unwrap();
        let default = registry["default_workspace"].as_str().unwrap();
        assert_eq!(
            registry["workspaces"][default]["root"],
            legacy.to_str().unwrap()
        );
        assert_eq!(
            registry["workspaces"].as_object().unwrap().len(),
            if action == "repair" { 1 } else { 2 }
        );
        assert!(todo(&legacy).is_file());
        if let Some(original) = original {
            assert_eq!(
                fs::read(requested.join(".config/workspace.json")).unwrap(),
                original
            );
        }
        if action != "repair" {
            assert!(todo(&requested).is_file());
        }
        assert_eq!(recorded(&f), env!("CARGO_PKG_VERSION"));
    }
}

#[test]
fn setup_earlier_migration_failure_does_not_execute_preparation_or_stamp() {
    let mut f = Fixture::new();
    let root = f.add("alpha");
    fs::remove_file(root.join(".config/workspace.json")).unwrap();
    fs::write(root.join(".brain"), "earlier hook obstruction").unwrap();
    stamp(&f, "0.70.0");
    let before = fs::read(f.registry_path()).unwrap();
    let output = repair(&f);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!root.join(".config/workspace.json").exists());
    assert_eq!(fs::read(f.registry_path()).unwrap(), before);
    assert_eq!(recorded(&f), "0.70.0");
}

#[test]
fn setup_committed_then_foreign_refresh_failure_is_partial_and_never_stamped() {
    for version in [env!("CARGO_PKG_VERSION"), "0.102.25"] {
        let mut f = Fixture::new();
        let root = f.add("alpha");
        let other = f.add("beta");
        fs::remove_file(root.join(".config/workspace.json")).unwrap();
        fs::write(
            other.join(".config/workspace.json"),
            "invalid foreign identity",
        )
        .unwrap();
        stamp(&f, version);
        let output = repair(&f);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("setup committed"));
        assert!(root.join(".config/workspace.json").is_file());
        assert_eq!(
            fs::read_to_string(other.join(".config/workspace.json")).unwrap(),
            "invalid foreign identity"
        );
        assert_eq!(recorded(&f), version);
    }
}

#[test]
fn setup_rejects_invalid_target_identity_without_replacement_or_success_stamp() {
    for kind in ["mismatch", "malformed", "newer"] {
        let mut f = Fixture::new();
        let root = f.add("alpha");
        let path = root.join(".config/workspace.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        match kind {
            "mismatch" => {
                manifest["workspace_id"] = serde_json::json!(brain::workspace::WorkspaceId::new());
            }
            "newer" => manifest["minimum_brain_version"] = "99.0.0".into(),
            _ => manifest = serde_json::json!({"invalid":true}),
        }
        let original = serde_json::to_vec(&manifest).unwrap();
        fs::write(&path, &original).unwrap();
        stamp(&f, "0.102.25");
        assert!(!repair(&f).status.success());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(recorded(&f), "0.102.25");
        assert!(!todo(&root).exists());
    }
}

#[test]
fn setup_does_not_recreate_an_unavailable_peer_root_or_cache() {
    let mut f = Fixture::new();
    let root = f.add("alpha");
    let other = f.add("beta");
    fs::remove_file(root.join(".config/workspace.json")).unwrap();
    fs::rename(&other, f.root("parked-beta")).unwrap();
    assert_ok(&repair(&f));
    assert!(!other.exists());
    assert!(!f.cache("beta").exists());
}

#[test]
fn ordinary_commands_still_fail_before_dispatch_with_invalid_identity() {
    let mut f = Fixture::new();
    let root = f.add("alpha");
    fs::remove_file(root.join(".config/workspace.json")).unwrap();
    let output = f.run(&["config", "set", "enable_triage_habits=false", "-w", "alpha"]);
    assert!(!output.status.success());
    assert!(!root.join(".config/workspace.json").exists());
    assert!(
        !fs::read_to_string(root.join(".config/config.json"))
            .unwrap()
            .contains("enable_triage_habits")
    );
}

#[test]
fn incomplete_setup_request_does_not_migrate_legacy_registry_or_stamp() {
    let f = Fixture::new();
    let root = f.root("legacy");
    fs::create_dir(&root).unwrap();
    let flat = serde_json::to_vec(&serde_json::json!({"root":root})).unwrap();
    fs::write(f.registry_path(), &flat).unwrap();
    stamp(&f, "0.101.0");
    let output = f
        .command()
        .args(["workspace", "create"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(f.registry_path()).unwrap(), flat);
    assert_eq!(recorded(&f), "0.101.0");
    assert!(!root.join(".config/workspace.json").exists());
    assert!(!f.root("config/brain/env.json.legacy-backup").exists());
}

#[test]
fn completed_workspace_migration_remains_committed_when_peer_compatibility_fails() {
    let mut f = Fixture::new();
    let root = f.add("alpha");
    let peer = f.add("beta");
    fs::remove_file(root.join(".config/workspace.json")).unwrap();
    fs::create_dir(root.join("tasks")).unwrap();
    fs::write(
        root.join("tasks/tasks.csv"),
        "task_id,task_name,assigned_to\nT1,Example,test-user\n",
    )
    .unwrap();
    fs::write(
        root.join("tasks/habits.csv"),
        "task_id,task_name,assigned_to\n",
    )
    .unwrap();
    fs::write(root.join("tasks/SCHEMA.json"), b"{}\n").unwrap();
    fs::write(
        root.join(".config/personalization.json"),
        b"{\"name\":\"Test User\"}\n",
    )
    .unwrap();
    fs::write(
        peer.join(".config/workspace.json"),
        "invalid foreign identity",
    )
    .unwrap();
    stamp(&f, "0.102.25");
    let output = f.run(&["workspace", "migrate", "-w", "alpha"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("setup committed"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join(".config/workspace.json").is_file());
    assert!(root.join(".config/users.json").is_file());
    let schema: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("tasks/SCHEMA.json")).unwrap()).unwrap();
    assert_eq!(schema["task_schema_version"], 2);
    assert_eq!(recorded(&f), "0.102.25");
}

#[test]
fn exhausted_automatic_install_wait_preserves_machine_stamp_and_managed_state() {
    use fs2::FileExt as _;
    let mut f = Fixture::new();
    let root = f.add("alpha");
    assert_ok(&f.sync());
    let managed = f.cache("alpha").join("skills-install/managed.json");
    let state = f.cache("alpha").join("state.db");
    let before = (
        fs::read(&managed).unwrap(),
        fs::read(&state).unwrap(),
        fs::read(todo(&root)).unwrap(),
    );
    let owner = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.cache("alpha").join("skills-install/install.lock"))
        .unwrap();
    owner.try_lock_exclusive().unwrap();
    stamp(&f, "0.102.24");
    let output = f.run(&["server", "status"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("compatibility refresh failed"));
    assert_eq!(recorded(&f), "0.102.24");
    assert_eq!(
        (
            fs::read(managed).unwrap(),
            fs::read(state).unwrap(),
            fs::read(todo(&root)).unwrap()
        ),
        before
    );
}

#[test]
fn compatibility_wait_notice_is_plain_stderr_once_and_json_stdout_stays_one_object() {
    use fs2::FileExt as _;
    use std::io::{BufRead as _, Read as _};
    let mut f = Fixture::new();
    f.add("alpha");
    assert_ok(&f.sync());
    let owner = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.cache("alpha").join("skills-install/install.lock"))
        .unwrap();
    owner.try_lock_exclusive().unwrap();
    let mut child = f
        .command()
        .args(["tasks", "remove", "--operation-status", "missing", "--json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = std::io::BufReader::new(child.stderr.take().unwrap());
    let mut first = String::new();
    stderr.read_line(&mut first).unwrap();
    assert_eq!(
        first,
        "Waiting for another skill update to finish (up to 30 seconds).\n"
    );
    drop(owner);
    let output = child.wait_with_output().unwrap();
    let mut remainder = String::new();
    stderr.read_to_string(&mut remainder).unwrap();
    assert!(!remainder.contains("Waiting for another skill update"));
    assert!(!first.contains('\u{1b}'));
    assert_eq!(output.status.code(), Some(1));
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["code"], "operation_unavailable");
}
