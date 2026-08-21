use super::*;

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

#[test]
fn a_cat_that_succeeds_with_no_bytes_is_an_absent_manifest_not_a_broken_one() {
    // rclone `cat` of a missing object in an empty bucket exits 0 and prints
    // nothing. Parsing that as a manifest reported the remote as corrupt and
    // refused every sync, so a brand-new bucket could never be initialized.
    let mut calls = Vec::new();
    let observation = super::probe_remote_identity_with(&remote(), &mut |_env, args| {
        calls.push(args.to_vec());
        match args.first().map(String::as_str) {
            // Both an absent object and an empty listing read as no bytes.
            Some("cat" | "lsf") => output(true, b"", ""),
            other => panic!("unexpected remote command {other:?}"),
        }
    })
    .expect("probe an empty remote");

    assert_eq!(observation, RemoteIdentityObservation::Empty);
    // It must still consult the listing rather than trusting the empty read.
    assert!(
        calls
            .iter()
            .any(|args| args.first().map(String::as_str) == Some("lsf")),
        "{calls:?}"
    );
}

#[test]
fn a_blank_manifest_object_is_also_treated_as_absent() {
    // Whitespace carries no ownership claim either; the listing decides.
    let observation = super::probe_remote_identity_with(&remote(), &mut |_env, args| match args
        .first()
        .map(String::as_str)
    {
        Some("cat") => output(true, b"\n  \n", ""),
        Some("lsf") => output(true, b"notes/plan.md\n", ""),
        other => panic!("unexpected remote command {other:?}"),
    })
    .expect("probe a remote with data");

    // Data but no manifest: adoption stays explicit, exactly as before.
    assert_eq!(observation, RemoteIdentityObservation::ManifestlessNonempty);
}

#[test]
fn a_nonempty_but_malformed_manifest_is_still_refused() {
    // Real bytes that do not parse could be a corrupted ownership claim, so
    // this must keep failing closed.
    let observation = super::probe_remote_identity_with(&remote(), &mut |_env, args| match args
        .first()
        .map(String::as_str)
    {
        Some("cat") => output(true, b"{ not json", ""),
        other => panic!("unexpected remote command {other:?}"),
    })
    .expect("probe a remote with a malformed manifest");

    assert!(matches!(
        observation,
        RemoteIdentityObservation::InvalidManifest { .. }
    ));
}
