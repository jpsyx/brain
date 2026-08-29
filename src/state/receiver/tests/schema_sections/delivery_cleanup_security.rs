#[cfg(unix)]
fn quarantine_contains_inode(parent: &std::path::Path, inode: u64) -> bool {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    std::fs::read_dir(parent)
        .expect("cleanup parent entries")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".brain-cleanup-"))
        .any(|entry| {
            std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(0o700))
                .expect("inspect private cleanup quarantine");
            std::fs::symlink_metadata(entry.path().join("artifact"))
                .is_ok_and(|metadata| metadata.ino() == inode)
        })
}

#[cfg(unix)]
#[test]
fn v12_down_rejects_symlinked_artifact_ancestors_then_cleans_exact_paths() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = canonical_temporary_state_path(&temporary, "workspace-cache/state.db");
    std::fs::create_dir_all(path.parent().expect("workspace cache"))
        .expect("workspace cache directory");
    let staged = stage_delivery_cleanup_down(&path, true);
    let responses = staged
        .response
        .parent()
        .expect("responses directory")
        .to_path_buf();
    let observations = staged
        .observation
        .parent()
        .expect("observations directory")
        .to_path_buf();
    let real_responses = responses.with_extension("real");
    let real_observations = observations.with_extension("real");
    std::fs::rename(&responses, &real_responses).expect("retain exact responses directory");
    std::fs::rename(&observations, &real_observations)
        .expect("retain exact observations directory");

    let outside = temporary.path().join("outside");
    let outside_responses = outside.join("responses");
    let outside_observations = outside.join("receiver-observations");
    std::fs::create_dir_all(&outside_responses).expect("outside responses");
    std::fs::create_dir_all(&outside_observations).expect("outside observations");
    let outside_response = outside_responses.join(format!("{}.json", staged.instance));
    let outside_observation = outside_observations.join(format!("{}.json", staged.instance));
    let outside_observation_lock = outside_observation.with_extension("json.lock");
    for artifact in [
        &outside_response,
        &outside_observation,
        &outside_observation_lock,
    ] {
        std::fs::write(artifact, "outside private artifact").expect("outside artifact");
    }
    symlink(&outside_responses, &responses).expect("responses ancestor symlink");
    symlink(&outside_observations, &observations).expect("observations ancestor symlink");

    super::super::schema::down_delivery_path(&path)
        .expect_err("symlinked cleanup ancestors must retain v12 authority");

    assert!(
        outside_response.exists()
            && outside_observation.exists()
            && outside_observation_lock.exists(),
        "downgrade deleted an outside artifact through a symlinked ancestor"
    );
    assert!(
        delivery_cleanup_down_state(&path, &staged) == (12, 1, 1, Some(42)),
        "downgrade discarded cleanup authority after a symlink rejection"
    );

    std::fs::remove_file(&responses).expect("remove responses ancestor symlink");
    std::fs::remove_file(&observations).expect("remove observations ancestor symlink");
    std::fs::rename(&real_responses, &responses).expect("restore exact responses directory");
    std::fs::rename(&real_observations, &observations)
        .expect("restore exact observations directory");

    super::super::schema::down_delivery_path(&path)
        .expect("retry exact cleanup and downgrade");

    assert!(
        delivery_cleanup_down_state(&path, &staged) == (11, 0, 0, None),
        "exact cleanup did not finish before downgrade"
    );
    assert!(!staged.response.exists());
    assert!(!staged.observation.exists());
    assert!(!staged.observation_lock.exists());
}

#[cfg(unix)]
#[test]
fn v12_down_treats_removal_after_open_as_exactly_absent() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = canonical_temporary_state_path(&temporary, "workspace-cache/state.db");
    std::fs::create_dir_all(path.parent().expect("workspace cache"))
        .expect("workspace cache directory");
    let staged = stage_delivery_cleanup_down(&path, true);
    let expected_relative = std::path::PathBuf::from("responses").join(
        staged
            .response
            .file_name()
            .expect("response artifact file name"),
    );
    let hook_response = staged.response.clone();

    crate::workspace::with_secure_remove_test_hook(
        move |boundary, relative| {
            if boundary == crate::workspace::SecureRemoveTestBoundary::OpenBeforeEntryStat
                && relative == expected_relative
                && hook_response.exists()
            {
                std::fs::remove_file(&hook_response)
                    .expect("remove response after descriptor open");
            }
        },
        || {
            super::super::schema::down_delivery_path(&path)
                .expect("exactly absent response must not block downgrade");
        },
    );

    assert!(
        delivery_cleanup_down_state(&path, &staged) == (11, 0, 0, None),
        "exactly absent response did not complete downgrade cleanup"
    );
    assert!(!staged.response.exists());
    assert!(!staged.observation.exists());
    assert!(!staged.observation_lock.exists());
}

#[cfg(unix)]
#[test]
fn v12_down_recovers_an_artifact_after_interruption_during_quarantine() {
    use std::os::unix::fs::MetadataExt as _;

    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = canonical_temporary_state_path(&temporary, "workspace-cache/state.db");
    std::fs::create_dir_all(path.parent().expect("workspace cache"))
        .expect("workspace cache directory");
    let staged = stage_delivery_cleanup_down(&path, true);
    let response_inode = std::fs::symlink_metadata(&staged.response)
        .expect("response metadata")
        .ino();
    let expected_relative = std::path::PathBuf::from("responses").join(
        staged
            .response
            .file_name()
            .expect("response artifact file name"),
    );

    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::workspace::with_secure_remove_test_hook(
            move |boundary, relative| {
                if boundary
                    == crate::workspace::SecureRemoveTestBoundary::QuarantineRenameBeforeVerification
                    && relative == expected_relative
                {
                    panic!("simulated downgrade cleanup interruption");
                }
            },
            || {
                super::super::schema::down_delivery_path(&path)
                    .expect("interruption must unwind downgrade");
            },
        );
    }));

    assert!(interrupted.is_err(), "downgrade interruption hook did not run");
    assert!(
        delivery_cleanup_down_state(&path, &staged) == (12, 1, 1, Some(42)),
        "interruption discarded downgrade cleanup authority"
    );
    assert!(!staged.response.exists());
    assert!(
        quarantine_contains_inode(
            staged.response.parent().expect("response directory"),
            response_inode,
        ),
        "interrupted downgrade did not retain the private artifact"
    );

    super::super::schema::down_delivery_path(&path)
        .expect("retry recovered quarantine and completed downgrade");

    assert!(
        delivery_cleanup_down_state(&path, &staged) == (11, 0, 0, None),
        "recovered quarantine did not complete downgrade"
    );
    assert!(
        !quarantine_contains_inode(
            staged.response.parent().expect("response directory"),
            response_inode,
        ),
        "downgrade retry orphaned the quarantined private artifact"
    );
}
