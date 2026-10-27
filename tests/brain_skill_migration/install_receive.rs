use super::{sync_fixture::Transfer, *};
use std::os::unix::fs::PermissionsExt as _;

fn install(transfer: &Transfer, mode: &str) -> Output {
    let f = &transfer.fixture;
    let source = f.temp.path().join("installer-source");
    let bin = f.temp.path().join("installed commands");
    std::fs::create_dir_all(source.join("target/release")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(source.join("Cargo.toml"), "[package]\nname = \"brain\"\n").unwrap();
    std::fs::copy(
        env!("CARGO_BIN_EXE_brain"),
        source.join("target/release/brain"),
    )
    .unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"),
        source.join("install.sh"),
    )
    .unwrap();
    let cargo = transfer.bin.join("cargo");
    std::fs::write(&cargo, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut command = transfer.command(mode);
    // Keep every real coordinator and transport operation, replacing only compilation.
    command = {
        let mut shell = Command::new("/bin/bash");
        shell.env_clear().envs(
            command
                .get_envs()
                .filter_map(|(key, value)| value.map(|v| (key, v))),
        );
        shell
    };
    command
        .arg(source.join("install.sh"))
        .args(["--name", "brain-custom"])
        .env("BIN_DIR", bin)
        .current_dir(f.temp.path())
        .output()
        .unwrap()
}

#[test]
fn installer_recovers_one_interrupted_pull_before_preflight_and_publication() {
    let transfer = Transfer::new();
    let _ = transfer.pull("fail");
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    let state = transfer
        .fixture
        .home
        .join(".config/brain/migrations/version");
    let before = std::fs::read(&state).unwrap();
    let output = install(&transfer, "complete");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(transfer.phase(), None);
    assert_eq!(transfer.transfers(), 2);
    assert_eq!(std::fs::read(state).unwrap(), before);
    assert!(
        transfer
            .fixture
            .temp
            .path()
            .join("installed commands/brain-custom")
            .is_file()
    );
    let output = install(&transfer, "fail");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        transfer.transfers(),
        2,
        "clean reinstall must not contact transport"
    );
}

fn prepare(transfer: &Transfer, mode: &str) -> Output {
    use sha2::{Digest as _, Sha256};
    let executable = env!("CARGO_BIN_EXE_brain");
    let digest = format!("{:x}", Sha256::digest(std::fs::read(executable).unwrap()));
    transfer.run(
        &[
            "__refresh-workspace-skills",
            "--target-binary",
            executable,
            "--target-version",
            env!("CARGO_PKG_VERSION"),
            "--phase",
            "prepare",
            "--expected-target-sha256",
            &digest,
        ],
        mode,
    )
}

#[test]
fn installer_offline_recovery_is_once_and_keeps_installed_binary_and_evidence() {
    let transfer = Transfer::new();
    let _ = transfer.pull("fail");
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    let bin = transfer.fixture.temp.path().join("installed commands");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_brain"), bin.join("brain-custom")).unwrap();
    let before = std::fs::read(bin.join("brain-custom")).unwrap();
    for attempts in [2, 3] {
        let failed = install(&transfer, "fail");
        assert!(!failed.status.success());
        assert_eq!(transfer.transfers(), attempts);
        assert_eq!(transfer.phase().as_deref(), Some("receiving"));
        assert_eq!(std::fs::read(bin.join("brain-custom")).unwrap(), before);
        let stderr = String::from_utf8_lossy(&failed.stderr);
        assert!(
            stderr.contains("alpha")
                && stderr.contains(&format!(
                    "'{}' sync --pull -w alpha",
                    bin.join("brain-custom").display()
                )),
            "{stderr}"
        );
        assert!(stderr.contains("rclone exited with an error"), "{stderr}");
    }
}

#[test]
fn installer_preparation_refuses_active_sync_without_transfer() {
    let transfer = Transfer::new();
    let _ = transfer.pull("fail");
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    let _owner =
        brain::sync::lock::try_acquire(&transfer.fixture.cache().join("sync/sync.lock")).unwrap();
    let output = prepare(&transfer, "complete");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("sync is in progress"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(transfer.transfers(), 1);
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
}

#[test]
fn installer_preparation_keeps_conflicts_and_does_not_pull_completed_receives() {
    let transfer = Transfer::new();
    super::sync_arrival::receiving(&transfer.fixture, "reconcile");
    let conflict = transfer
        .fixture
        .root
        .join("note (conflict host 2026-10-25).md");
    std::fs::write(&conflict, "preserve both versions").unwrap();
    let output = prepare(&transfer, "complete");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("conflict"));
    assert_eq!(transfer.transfers(), 0);
    assert_eq!(transfer.phase().as_deref(), Some("reconcile"));
    assert_eq!(
        std::fs::read_to_string(conflict).unwrap(),
        "preserve both versions"
    );
}

#[test]
fn installer_preparation_reconciles_completed_receive_without_network() {
    let transfer = Transfer::new();
    super::sync_arrival::receiving(&transfer.fixture, "reconcile");
    let output = prepare(&transfer, "fail");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(transfer.transfers(), 0);
    assert_eq!(transfer.phase(), None);
}

#[test]
fn installer_preparation_never_escalates_a_pull_to_resync_or_repair() {
    for mode in ["baseline", "check-access"] {
        let transfer = Transfer::new();
        let _ = transfer.pull("fail");
        assert_eq!(transfer.phase().as_deref(), Some("receiving"));
        let output = prepare(&transfer, mode);
        assert!(!output.status.success());
        assert_eq!(
            transfer.transfers(),
            2,
            "{mode}: automatic preparation must attempt one pull only"
        );
        assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    }
}

#[test]
fn installer_preparation_clean_workspace_is_read_only_and_offline() {
    let transfer = Transfer::new();
    let f = &transfer.fixture;
    let state = f.home.join(".config/brain/migrations/version");
    let manifest = f.cache().join("skills-install/managed.json");
    let before = [
        std::fs::read(&state).unwrap(),
        std::fs::read(&manifest).unwrap(),
    ];
    let output = prepare(&transfer, "fail");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!transfer.trace.exists());
    assert_eq!(std::fs::read(state).unwrap(), before[0]);
    assert_eq!(std::fs::read(manifest).unwrap(), before[1]);
}

#[test]
fn installer_can_resume_after_a_second_interruption() {
    let transfer = Transfer::new();
    let _ = transfer.pull("fail");
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    assert!(!install(&transfer, "crash").status.success());
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    assert_eq!(transfer.transfers(), 2);
    let output = install(&transfer, "complete");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(transfer.transfers(), 3);
    assert_eq!(transfer.phase(), None);
}

#[test]
fn installer_recovery_transport_conflict_stays_blocked_on_retry() {
    let transfer = Transfer::new();
    let _ = transfer.pull("fail");
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    let output = prepare(&transfer, "conflict");
    assert!(!output.status.success());
    assert_eq!(transfer.phase().as_deref(), Some("reconcile"));
    assert_eq!(transfer.transfers(), 2);
    let output = prepare(&transfer, "complete");
    assert!(!output.status.success());
    assert_eq!(transfer.phase().as_deref(), Some("reconcile"));
    assert_eq!(transfer.transfers(), 2);
}

#[test]
fn installer_preparation_validates_local_and_remote_workspace_identity() {
    for local in [true, false] {
        let transfer = Transfer::new();
        let _ = transfer.pull("fail");
        assert_eq!(transfer.phase().as_deref(), Some("receiving"));
        let root = if local {
            &transfer.fixture.root
        } else {
            &transfer.remote
        };
        let path = root.join(".config/workspace.json");
        let mut manifest = read_json(&path);
        manifest["workspace_id"] = json!("22222222-2222-4222-8222-222222222222");
        write_json(&path, &manifest);
        let output = prepare(&transfer, "complete");
        assert!(!output.status.success());
        assert_eq!(transfer.transfers(), 1);
        assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    }
}

#[test]
fn unavailable_downgrade_root_refuses_before_any_receive_preparation() {
    use sha2::{Digest as _, Sha256};
    let transfer = Transfer::new();
    let f = &transfer.fixture;
    let _ = transfer.pull("fail");
    super::installer::compatible(f);
    let path = f.home.join(".config/brain/env.json");
    let mut registry = read_json(&path);
    registry["workspaces"]["unavailable"] = json!({"workspace_id":"22222222-2222-4222-8222-222222222222", "root":f.temp.path().join("absent-root"), "local_user_id":"member", "env":{}});
    write_json(&path, &registry);
    let target = super::installer::target(f, "0.101.0");
    let digest = format!("{:x}", Sha256::digest(std::fs::read(&target).unwrap()));
    let output = transfer.run(
        &[
            "__refresh-workspace-skills",
            "--target-binary",
            target.to_str().unwrap(),
            "--target-version",
            "0.101.0",
            "--phase",
            "prepare",
            "--expected-target-sha256",
            &digest,
        ],
        "complete",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("pending_unavailable"));
    assert_eq!(transfer.transfers(), 1);
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
}

#[test]
fn receive_preparation_does_not_migrate_a_rejected_target_or_unaffected_root() {
    let transfer = Transfer::new();
    let f = &transfer.fixture;
    let _ = transfer.pull("fail");
    super::installer::compatible(f);
    let path = f.home.join(".config/brain/env.json");
    let mut registry = read_json(&path);
    let other = f.temp.path().join("other-root");
    let id = brain::workspace::WorkspaceId::parse("22222222-2222-4222-8222-222222222222").unwrap();
    brain::workspace::WorkspaceManifest::new(id)
        .write_new(&other)
        .unwrap();
    std::fs::write(other.join("AGENTS.md"), "unaffected instructions\n").unwrap();
    registry["workspaces"]["other"] =
        json!({"workspace_id":id, "root":other, "local_user_id":"member", "env":{}});
    write_json(&path, &registry);
    let state = f.home.join(".config/brain/migrations/version");
    let before = std::fs::read(&state).unwrap();
    let output = prepare(&transfer, "complete");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(transfer.transfers(), 2);
    assert_eq!(
        std::fs::read_to_string(other.join("AGENTS.md")).unwrap(),
        "unaffected instructions\n"
    );
    assert!(!other.join(".agents").exists());
    // The target's schema is incompatible, but preparation must not stamp or seed it.
    let schema = f.root.join("tasks/SCHEMA.json");
    std::fs::write(&schema, br#"{"task_schema_version":99}"#).unwrap();
    let target = super::installer::target(f, "0.101.0");
    let failed = super::installer::refresh(f, &target, "0.101.0", "preflight");
    assert!(!failed.status.success());
    assert_eq!(std::fs::read(&state).unwrap(), before);
    assert_eq!(
        std::fs::read(&schema).unwrap(),
        br#"{"task_schema_version":99}"#
    );
}

#[test]
fn preparation_reports_invalid_sync_configuration_without_echoing_its_values() {
    let transfer = Transfer::new();
    super::sync_arrival::receiving(&transfer.fixture, "receiving");
    let path = transfer.fixture.home.join(".config/brain/env.json");
    let mut registry = read_json(&path);
    registry["workspaces"]["alpha"]["env"]["sync"]["enabled"] = json!("PRIVATE_FIXTURE_VALUE");
    write_json(&path, &registry);
    let output = prepare(&transfer, "complete");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid workspace sync configuration"),
        "{stderr}"
    );
    assert!(!stderr.contains("PRIVATE_FIXTURE_VALUE"), "{stderr}");
    assert_eq!(transfer.phase().as_deref(), Some("receiving"));
    assert_eq!(transfer.transfers(), 0);
}

#[test]
fn preparation_reports_invalid_manifest_without_echoing_parser_values() {
    let transfer = Transfer::new();
    super::sync_arrival::receiving(&transfer.fixture, "receiving");
    let path = transfer.fixture.root.join(".config/workspace.json");
    let mut manifest = read_json(&path);
    manifest["minimum_brain_version"] = json!("PRIVATE_FIXTURE_VALUE");
    write_json(&path, &manifest);
    let output = prepare(&transfer, "complete");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid_source"), "{stderr}");
    assert!(!stderr.contains("PRIVATE_FIXTURE_VALUE"), "{stderr}");
    assert_eq!(transfer.transfers(), 0);
}
