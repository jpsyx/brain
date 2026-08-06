include!("tests_support.rs");

#[test]
fn setup_publishes_the_existing_local_manifest_first_and_verifies_readback() {
    let root = tempfile::tempdir().unwrap();
    let bytes = manifest_bytes(PERSONAL_ID);
    write_manifest(root.path(), &bytes);
    let calls = RefCell::new(Vec::<Vec<String>>::new());
    let mut step = 0;

    ensure_remote_identity_for_setup_with(
        root.path(),
        workspace_id(PERSONAL_ID),
        &remote(),
        |_| Ok(ManifestlessRemoteAdoption::Refuse),
        |_, args| {
            calls.borrow_mut().push(args.to_vec());
            let response = match step {
                0 | 5 => output(false, b"", "object not found"),
                1 | 6 | 7 => output(true, b"", ""),
                2 | 4 | 8 => output(true, &bytes, ""),
                3 => output(true, format!("{PERSONAL_ID}.json\n").as_bytes(), ""),
                _ => panic!("unexpected remote command"),
            };
            step += 1;
            response
        },
    )
    .unwrap();

    let calls = calls.into_inner();
    assert_eq!(
        &calls[0][..2],
        ["cat", "BRAIN:shared/brain/.config/workspace.json"]
    );
    assert_eq!(calls[1][0], "lsf");
    assert_eq!(
        &calls[7][..3],
        [
            "copyto",
            WorkspaceManifest::path(root.path())
                .to_string_lossy()
                .as_ref(),
            "BRAIN:shared/brain/.config/workspace.json",
        ]
    );
    assert_eq!(
        &calls[8][..2],
        ["cat", "BRAIN:shared/brain/.config/workspace.json"]
    );
    assert_eq!(
        std::fs::read(WorkspaceManifest::path(root.path())).unwrap(),
        bytes
    );
}

#[test]
fn first_setup_attempt_stages_a_new_claim_without_publishing_canonical_identity() {
    let root = tempfile::tempdir().unwrap();
    let bytes = manifest_bytes(PERSONAL_ID);
    write_manifest(root.path(), &bytes);
    let canonical_publications = Cell::new(0);
    let mut step = 0;

    let error = ensure_remote_identity_for_setup_with(
        root.path(),
        workspace_id(PERSONAL_ID),
        &remote(),
        |_| Ok(ManifestlessRemoteAdoption::Refuse),
        |_, args| {
            let response = match step {
                0 | 2 | 7 => output(false, b"", "object not found"),
                1 | 3 | 8 => output(true, b"", ""),
                4 | 6 | 10 => output(true, &bytes, ""),
                5 => output(true, format!("{PERSONAL_ID}.json\n").as_bytes(), ""),
                _ if args.first().map(String::as_str) == Some("copyto") => {
                    canonical_publications.set(canonical_publications.get() + 1);
                    output(true, b"", "")
                }
                _ => output(true, format!("{PERSONAL_ID}.json\n").as_bytes(), ""),
            };
            step += 1;
            response
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("claim staged"), "{error:#}");
    assert_eq!(canonical_publications.get(), 0);
}

#[test]
fn late_competing_claims_stage_then_only_one_retry_publishes_with_non_atomic_copy() {
    let personal = tempfile::tempdir().unwrap();
    let family = tempfile::tempdir().unwrap();
    write_manifest(personal.path(), &manifest_bytes(PERSONAL_ID));
    write_manifest(family.path(), &manifest_bytes(FAMILY_ID));
    let state = Arc::new(RaceRemote::default());

    let first_attempts = std::thread::scope(|scope| {
        let launch = |root: &Path, id: WorkspaceId| {
            let root = root.to_path_buf();
            let state = Arc::clone(&state);
            scope.spawn(move || {
                ensure_remote_identity_for_setup_with(
                    &root,
                    id,
                    &remote(),
                    |_| Ok(ManifestlessRemoteAdoption::Refuse),
                    |_, args| state.run(args),
                )
                .map(|_| ())
            })
        };
        let personal = launch(personal.path(), workspace_id(PERSONAL_ID));
        let family = launch(family.path(), workspace_id(FAMILY_ID));
        [personal.join().unwrap(), family.join().unwrap()]
    });

    assert!(first_attempts.iter().all(Result::is_err));
    assert!(first_attempts.iter().all(|result| {
        result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("claim staged")
    }));
    assert_eq!(state.snapshot(), (0, None));

    let retries = std::thread::scope(|scope| {
        let launch = |root: &Path, id: WorkspaceId| {
            let root = root.to_path_buf();
            let state = Arc::clone(&state);
            scope.spawn(move || {
                ensure_remote_identity_for_setup_with(
                    &root,
                    id,
                    &remote(),
                    |_| Ok(ManifestlessRemoteAdoption::Refuse),
                    |_, args| state.run(args),
                )
                .map(|_| ())
            })
        };
        let personal = launch(personal.path(), workspace_id(PERSONAL_ID));
        let family = launch(family.path(), workspace_id(FAMILY_ID));
        [personal.join().unwrap(), family.join().unwrap()]
    });

    assert_eq!(retries.iter().filter(|result| result.is_ok()).count(), 1);
    let (manifest_publications, manifest) = state.snapshot();
    assert_eq!(manifest_publications, 1);
    assert_eq!(
        manifest.as_deref(),
        Some(manifest_bytes(PERSONAL_ID).as_slice()),
        "the deterministic lowest UUID claim owns the remote"
    );
}

#[test]
fn setup_refuses_a_mismatched_remote_before_any_publication() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(root.path(), &manifest_bytes(PERSONAL_ID));
    let remote_bytes = manifest_bytes(FAMILY_ID);
    let calls = RefCell::new(Vec::<Vec<String>>::new());

    let error = ensure_remote_identity_for_setup_with(
        root.path(),
        workspace_id(PERSONAL_ID),
        &remote(),
        |_| Ok(ManifestlessRemoteAdoption::Refuse),
        |_, args| {
            calls.borrow_mut().push(args.to_vec());
            output(true, &remote_bytes, "")
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains(PERSONAL_ID), "{error:#}");
    assert!(error.to_string().contains(FAMILY_ID), "{error:#}");
    assert_eq!(
        calls.into_inner().len(),
        1,
        "mismatch must stop before copyto"
    );
}

#[test]
fn setup_refuses_a_nonempty_manifestless_remote_without_publication() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(root.path(), &manifest_bytes(PERSONAL_ID));
    let calls = RefCell::new(Vec::<Vec<String>>::new());
    let mut step = 0;

    let error = ensure_remote_identity_for_setup_with(
        root.path(),
        workspace_id(PERSONAL_ID),
        &remote(),
        |_| Ok(ManifestlessRemoteAdoption::Refuse),
        |_, args| {
            calls.borrow_mut().push(args.to_vec());
            let response = match step {
                0 => output(false, b"", "object not found"),
                1 => output(true, b"notes.md\n", ""),
                _ => panic!("manifestless nonempty remote must stop before publication"),
            };
            step += 1;
            response
        },
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("has data but no workspace manifest"),
        "{error:#}"
    );
    assert_eq!(calls.into_inner().len(), 2);
}

#[test]
fn setup_adopts_a_nonempty_manifestless_remote_only_with_exact_authority() {
    let root = tempfile::tempdir().unwrap();
    let bytes = manifest_bytes(PERSONAL_ID);
    write_manifest(root.path(), &bytes);
    let calls = RefCell::new(Vec::<Vec<String>>::new());
    let observations = RefCell::new(Vec::new());
    let mut step = 0;
    let remote = remote();

    let verified = ensure_remote_identity_for_setup_with(
        root.path(),
        workspace_id(PERSONAL_ID),
        &remote,
        |observed| {
            observations.borrow_mut().push(observed.clone());
            Ok(ManifestlessRemoteAdoption::Authorized(workspace_id(
                PERSONAL_ID,
            )))
        },
        |_, args| {
            calls.borrow_mut().push(args.to_vec());
            let response = match step {
                0 | 5 => output(false, b"", "object not found"),
                1 | 6 => output(true, b"notes.md\n", ""),
                2 | 4 | 8 => output(true, &bytes, ""),
                3 => output(true, format!("{PERSONAL_ID}.json\n").as_bytes(), ""),
                7 => output(true, b"", ""),
                _ => panic!("unexpected remote command"),
            };
            step += 1;
            response
        },
    )
    .expect("exact authority adopts the target");

    assert_eq!(verified.remote(), &remote);
    assert_eq!(
        observations.into_inner(),
        [RemoteIdentityObservation::ManifestlessNonempty]
    );
    let calls = calls.into_inner();
    assert_eq!(calls[0][0], "cat");
    assert_eq!(calls[1][0], "lsf");
    assert_eq!(calls[7][0], "copyto");
    assert_eq!(calls[8][0], "cat");
}

#[test]
fn setup_never_adopts_when_the_listing_contains_an_unreadable_manifest() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(root.path(), &manifest_bytes(PERSONAL_ID));
    let calls = RefCell::new(Vec::<Vec<String>>::new());
    let observations = RefCell::new(Vec::new());
    let mut step = 0;

    let error = ensure_remote_identity_for_setup_with(
        root.path(),
        workspace_id(PERSONAL_ID),
        &remote(),
        |observed| {
            observations.borrow_mut().push(observed.clone());
            Ok(ManifestlessRemoteAdoption::Authorized(workspace_id(
                PERSONAL_ID,
            )))
        },
        |_, args| {
            calls.borrow_mut().push(args.to_vec());
            let response = match step {
                0 => output(false, b"", "temporary read failure"),
                1 => output(true, b".config/workspace.json\nnotes.md\n", ""),
                _ => panic!("an unreadable manifest must stop before publication"),
            };
            step += 1;
            response
        },
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("manifest is present but unreadable")
    );
    assert!(error.to_string().contains("temporary read failure"));
    assert!(matches!(
        observations.into_inner().as_slice(),
        [RemoteIdentityObservation::UnreadableManifest { .. }]
    ));
    assert_eq!(calls.into_inner().len(), 2);
}

#[test]
fn ordinary_gate_refuses_an_empty_uninitialized_remote() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(root.path(), &manifest_bytes(PERSONAL_ID));
    let mut step = 0;

    let error =
        require_remote_identity_with(root.path(), workspace_id(PERSONAL_ID), &remote(), |_, _| {
            let response = match step {
                0 => output(false, b"", "object not found"),
                1 => output(true, b"", ""),
                _ => panic!("ordinary gate must never initialize"),
            };
            step += 1;
            response
        })
        .unwrap_err();

    assert!(error.to_string().contains("not initialized"), "{error:#}");
}

#[test]
fn local_validation_refuses_record_manifest_mismatch_without_rewriting_bytes() {
    let root = tempfile::tempdir().unwrap();
    let bytes = manifest_bytes(FAMILY_ID);
    write_manifest(root.path(), &bytes);

    let error = validate_local_manifest(root.path(), workspace_id(PERSONAL_ID)).unwrap_err();

    assert!(error.to_string().contains(PERSONAL_ID), "{error:#}");
    assert!(error.to_string().contains(FAMILY_ID), "{error:#}");
    assert_eq!(
        std::fs::read(WorkspaceManifest::path(root.path())).unwrap(),
        bytes
    );
}
