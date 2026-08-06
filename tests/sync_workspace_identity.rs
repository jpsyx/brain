include!("support/sync_workspace_identity_support.rs");

#[test]
fn selected_sync_config_never_reads_another_workspace_record() {
    let fixture = Fixture::new();

    let personal = SyncConfig::load(&fixture.personal);
    let family = SyncConfig::load(&fixture.family);

    assert_eq!(personal.b2_bucket, "personal-bucket");
    assert_eq!(personal.b2_key_id, "personal-bucket-id");
    assert_eq!(personal.b2_app_key, "personal-key");
    assert_eq!(family.b2_bucket, "family-bucket");
    assert_eq!(family.b2_key_id, "family-bucket-id");
    assert_eq!(family.b2_app_key, "family-key");
}

#[test]
fn direct_remote_identity_comparison_initializes_matches_and_refuses_mismatch() {
    let personal = workspace_id(PERSONAL_ID);
    let family = workspace_id(FAMILY_ID);

    assert_eq!(
        check_remote_identity(personal, None),
        RemoteIdentityDecision::Initialize
    );
    assert_eq!(
        check_remote_identity(personal, Some(personal)),
        RemoteIdentityDecision::Proceed
    );
    assert_eq!(
        check_remote_identity(personal, Some(family)),
        RemoteIdentityDecision::RefuseMismatch {
            local: personal,
            remote: family,
        }
    );
}

#[test]
fn manifest_decision_refuses_a_nonempty_remote_without_an_identity() {
    assert_eq!(
        check_remote_manifest_identity(
            workspace_id(PERSONAL_ID),
            None,
            false,
            env!("CARGO_PKG_VERSION"),
        ),
        RemoteIdentityDecision::RefuseMissingManifest
    );
}

#[test]
fn manifest_decision_accepts_only_a_matching_compatible_manifest() {
    let local = workspace_id(PERSONAL_ID);
    let matching = manifest_bytes(PERSONAL_ID, "0.1.0");
    let mismatched = manifest_bytes(FAMILY_ID, "0.1.0");

    assert_eq!(
        check_remote_manifest_identity(local, Some(&matching), false, env!("CARGO_PKG_VERSION")),
        RemoteIdentityDecision::Proceed
    );
    assert_eq!(
        check_remote_manifest_identity(local, Some(&mismatched), false, env!("CARGO_PKG_VERSION"),),
        RemoteIdentityDecision::RefuseMismatch {
            local,
            remote: workspace_id(FAMILY_ID),
        }
    );
}

#[test]
fn manifest_decision_refuses_malformed_and_incompatible_bytes() {
    let local = workspace_id(PERSONAL_ID);
    let incompatible = manifest_bytes(PERSONAL_ID, "999.0.0");

    assert!(matches!(
        check_remote_manifest_identity(local, Some(b"not json"), false, env!("CARGO_PKG_VERSION")),
        RemoteIdentityDecision::RefuseInvalidManifest { .. }
    ));
    assert!(matches!(
        check_remote_manifest_identity(
            local,
            Some(&incompatible),
            false,
            env!("CARGO_PKG_VERSION"),
        ),
        RemoteIdentityDecision::RefuseInvalidManifest { .. }
    ));
}

#[test]
fn two_records_targeting_the_same_remote_cannot_cross_adopt() {
    let fixture = Fixture::new();
    let mut personal = SyncConfig::load(&fixture.personal);
    let mut family = SyncConfig::load(&fixture.family);
    personal.b2_bucket = "shared-bucket".to_owned();
    family.b2_bucket = "shared-bucket".to_owned();
    personal.b2_path = "one-root".to_owned();
    family.b2_path = "one-root".to_owned();
    let personal_remote = brain::sync::remote::build_remote(&personal);
    let family_remote = brain::sync::remote::build_remote(&family);
    let remote_manifest = manifest_bytes(PERSONAL_ID, "0.1.0");

    assert_eq!(personal_remote.arg, family_remote.arg);
    assert_eq!(
        check_remote_manifest_identity(
            fixture.personal.workspace.id(),
            Some(&remote_manifest),
            false,
            env!("CARGO_PKG_VERSION"),
        ),
        RemoteIdentityDecision::Proceed
    );
    assert_eq!(
        check_remote_manifest_identity(
            fixture.family.workspace.id(),
            Some(&remote_manifest),
            false,
            env!("CARGO_PKG_VERSION"),
        ),
        RemoteIdentityDecision::RefuseMismatch {
            local: workspace_id(FAMILY_ID),
            remote: workspace_id(PERSONAL_ID),
        }
    );
}

#[test]
fn local_rclone_initializes_only_an_empty_remote_and_verifies_exact_bytes() {
    if !std::process::Command::new("rclone")
        .arg("version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        eprintln!("skipping: rclone not on PATH");
        return;
    }
    let fixture = Fixture::new();
    let remote_dir = fixture.home.path().join("remote");
    std::fs::create_dir_all(&remote_dir).unwrap();
    let remote = brain::sync::remote::Remote {
        env: Vec::new(),
        arg: remote_dir.display().to_string(),
    };
    let local_path = brain::workspace::WorkspaceManifest::path(fixture.personal.workspace.root());
    let before = std::fs::read(&local_path).unwrap();

    let staged = brain::sync::identity::ensure_remote_identity_for_setup(
        fixture.personal.workspace.root(),
        fixture.personal.workspace.id(),
        &remote,
    )
    .unwrap_err();

    assert!(staged.to_string().contains("claim staged"), "{staged:#}");
    assert!(!remote_dir.join(".config/workspace.json").exists());
    assert_eq!(std::fs::read(&local_path).unwrap(), before);

    brain::sync::identity::ensure_remote_identity_for_setup(
        fixture.personal.workspace.root(),
        fixture.personal.workspace.id(),
        &remote,
    )
    .unwrap();

    assert_eq!(std::fs::read(&local_path).unwrap(), before);
    assert_eq!(
        std::fs::read(remote_dir.join(".config/workspace.json")).unwrap(),
        before
    );
    let error = brain::sync::identity::require_remote_identity(
        fixture.family.workspace.root(),
        fixture.family.workspace.id(),
        &remote,
    )
    .unwrap_err();
    assert!(error.to_string().contains(PERSONAL_ID), "{error:#}");
    assert!(error.to_string().contains(FAMILY_ID), "{error:#}");
}

#[cfg(unix)]
#[test]
fn sync_repair_and_check_refuse_mismatched_remote_before_any_data_command() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::process::Command;

    let fixture = Fixture::new();
    let bin_dir = fixture.home.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let rclone = bin_dir.join("rclone");
    std::fs::write(
        &rclone,
        b"#!/bin/sh\ncase \"$1\" in\n  version) exit 0 ;;\n  cat) printf '%s' \"$REMOTE_MANIFEST\"; exit 0 ;;\n  *) printf '%s\\n' \"$*\" >> \"$RCLONE_DATA_LOG\"; exit 97 ;;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(&rclone, std::fs::Permissions::from_mode(0o700)).unwrap();
    let data_log = fixture.home.path().join("rclone-data.log");
    let remote_manifest = String::from_utf8(manifest_bytes(FAMILY_ID, "0.1.0")).unwrap();
    let bisync_workdir = brain::sync::run::bisync_workdir(fixture.personal.workspace.paths());

    for args in [vec!["sync"], vec!["sync", "repair"], vec!["check"]] {
        let _ = std::fs::remove_file(&data_log);
        let _ = std::fs::remove_dir_all(&bisync_workdir);
        let output = Command::new(env!("CARGO_BIN_EXE_brain"))
            .args(&args)
            .env("HOME", fixture.home.path())
            .env("XDG_CONFIG_HOME", fixture.home.path().join("config"))
            .env("PATH", format!("{}:/usr/bin:/bin", bin_dir.display()))
            .env("NO_COLOR", "1")
            .env("REMOTE_MANIFEST", &remote_manifest)
            .env("RCLONE_DATA_LOG", &data_log)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            !output.status.success(),
            "brain {args:?} unexpectedly succeeded"
        );
        assert!(stderr.contains(PERSONAL_ID), "brain {args:?}: {stderr}");
        assert!(stderr.contains(FAMILY_ID), "brain {args:?}: {stderr}");
        assert!(
            !data_log.exists(),
            "brain {args:?} reached a data command: {}",
            std::fs::read_to_string(&data_log).unwrap_or_default()
        );
        if args.first() == Some(&"sync") {
            assert!(
                !bisync_workdir.exists(),
                "brain {args:?} created UUID runtime workdir before remote identity"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn ordinary_sync_refuses_an_incomplete_schema_migration_before_rclone() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::process::Command;

    let fixture = Fixture::new();
    let journal = fixture.personal.workspace.paths().migration_journal();
    std::fs::create_dir_all(journal.parent().unwrap()).unwrap();
    std::fs::write(&journal, b"active migration\n").unwrap();
    let bin_dir = fixture.home.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let rclone = bin_dir.join("rclone");
    std::fs::write(
        &rclone,
        b"#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$RCLONE_LOG\"\nexit 97\n",
    )
    .unwrap();
    std::fs::set_permissions(&rclone, std::fs::Permissions::from_mode(0o700)).unwrap();
    let log = fixture.home.path().join("rclone.log");

    let output = Command::new(env!("CARGO_BIN_EXE_brain"))
        .arg("sync")
        .env("HOME", fixture.home.path())
        .env("XDG_CONFIG_HOME", fixture.home.path().join("config"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin_dir.display()))
        .env("RCLONE_LOG", &log)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "sync unexpectedly crossed migration"
    );
    assert!(
        stderr.contains("workspace migration is incomplete"),
        "{stderr}"
    );
    assert!(
        !log.exists(),
        "sync invoked rclone before migration resumed"
    );
}
