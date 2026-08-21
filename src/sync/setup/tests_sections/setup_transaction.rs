
#[test]
fn sync_block_preserves_existing_crypt_fields() {
    let existing = SyncConfig {
        b2_path: "prefix".to_owned(),
        crypt_password: "obscured-pass".to_owned(),
        crypt_password2: "obscured-salt".to_owned(),
        crypt_filename_encryption: "obfuscate".to_owned(),
        crypt_directory_name_encryption: false,
        ..SyncConfig::default()
    };

    let block = sync_block("bucket", "key-id", "app-key", &existing);

    assert_eq!(block["b2_bucket"], "bucket");
    assert_eq!(block["b2_key_id"], "key-id");
    assert_eq!(block["b2_app_key"], "app-key");
    assert_eq!(block["b2_path"], "prefix");
    assert_eq!(block["crypt_password"], "obscured-pass");
    assert_eq!(block["crypt_password2"], "obscured-salt");
    assert_eq!(block["crypt_filename_encryption"], "obfuscate");
    assert_eq!(block["crypt_directory_name_encryption"], false);
}

#[test]
fn setup_stages_verify_remote_identity_before_persisting_credentials_or_syncing_data() {
    let stages = RefCell::new(Vec::new());
    let temporary = tempfile::tempdir().unwrap();
    let paths = crate::workspace::WorkspacePaths::new(temporary.path(), local_workspace_id());

    run_setup_stages(
        &paths,
        || {
            stages.borrow_mut().push("identity");
            Ok(())
        },
        || {
            stages.borrow_mut().push("credentials");
            Ok(())
        },
        || {
            stages.borrow_mut().push("baseline");
            Ok(crate::sync::verify::Outcome::Clean)
        },
    )
    .unwrap();

    assert_eq!(*stages.borrow(), ["identity", "baseline", "credentials"]);
}

#[test]
fn setup_holds_the_uuid_lock_against_manual_sync_through_the_baseline() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = crate::workspace::WorkspacePaths::new(temporary.path(), local_workspace_id());
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);

    std::thread::scope(|scope| {
        let setup_paths = &paths;
        let setup = scope.spawn(move || {
            run_setup_stages(
                setup_paths,
                || {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                },
                || Ok(()),
                || Ok(crate::sync::verify::Outcome::Clean),
            )
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("setup reached identity while holding its lock");

        assert_eq!(
            crate::command::sync::run_with_workspace_lock(
                &paths,
                true,
                || panic!("manual sync entered while setup owned the workspace"),
                || panic!("if-idle sync must coalesce instead of follow"),
            ),
            crate::command::sync::WorkspaceLockOutcome::Coalesced,
        );
        release_tx.send(()).unwrap();
        setup.join().unwrap().unwrap();
    });
}

#[test]
fn migration_activation_blocks_setup_before_either_can_change_remote_state() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = crate::workspace::WorkspacePaths::new(temporary.path(), local_workspace_id());
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);

    std::thread::scope(|scope| {
        let migration_paths = &paths;
        let migration = scope.spawn(move || {
            crate::migration::with_activation_lock(migration_paths, || {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("migration reached discovery while holding its activation lock");
        let identity_entered = std::cell::Cell::new(false);

        let error = run_setup_stages(
            &paths,
            || {
                identity_entered.set(true);
                Ok(())
            },
            || Ok(()),
            || Ok(crate::sync::verify::Outcome::Clean),
        )
        .unwrap_err();

        assert!(error.to_string().contains("another sync owns"), "{error:#}");
        assert!(!identity_entered.get());
        release_tx.send(()).unwrap();
        migration.join().unwrap().unwrap();
    });
}
