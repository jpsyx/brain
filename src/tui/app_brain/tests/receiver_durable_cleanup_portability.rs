#[cfg(unix)]
use super::receiver_durable_answer_cleanup::answer_fixture;
#[cfg(unix)]
use super::receiver_durable_cleanup_security::{
    cleanup_authority_exists, quarantine_contains_inode,
};
#[cfg(unix)]
use super::receiver_durable_support::publish_valid_completion;
#[cfg(unix)]
use super::*;

#[cfg(unix)]
fn cleanup_quarantine(parent: &std::path::Path) -> std::path::PathBuf {
    std::fs::read_dir(parent)
        .expect("cleanup parent entries")
        .filter_map(Result::ok)
        .find(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(".brain-cleanup-") && !name.starts_with(".brain-cleanup-blocked-")
        })
        .expect("cleanup quarantine")
        .path()
}

#[cfg(unix)]
#[test]
fn runtime_recovers_a_new_quarantine_without_nofollow_path_chmod_support() {
    use std::os::unix::fs::MetadataExt as _;

    let (temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer retained across portable recovery");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    let artifact_inode = std::fs::symlink_metadata(&response)
        .expect("response metadata")
        .ino();
    let expected_relative = std::path::PathBuf::from("responses")
        .join(response.file_name().expect("response artifact file name"));

    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::workspace::with_secure_remove_test_hook(
            move |boundary, relative| {
                if boundary
                    == crate::workspace::SecureRemoveTestBoundary::QuarantineRenameBeforeVerification
                    && relative == expected_relative
                {
                    panic!("simulated portable cleanup interruption");
                }
            },
            || app.tick_receiver(),
        );
    }));
    assert!(
        interrupted.is_err(),
        "cleanup interruption hook did not run"
    );
    assert!(
        cleanup_authority_exists(&app, first.job_id()),
        "interruption discarded runtime cleanup authority"
    );
    assert!(
        cleanup_quarantine(response.parent().expect("response directory"))
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with("-pending")),
        "moved artifact quarantine was not durably pending"
    );
    assert!(
        quarantine_contains_inode(
            response.parent().expect("response directory"),
            artifact_inode
        ),
        "interruption did not retain the owner-private artifact"
    );
    drop(app);

    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(&temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    restarted
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    crate::workspace::with_unsupported_recovery_nofollow_chmod(|| {
        restarted.tick_receiver();
    });

    assert!(
        !cleanup_authority_exists(&restarted, first.job_id()),
        "unsupported nofollow path chmod stranded cleanup authority"
    );
    assert!(
        !quarantine_contains_inode(
            response.parent().expect("response directory"),
            artifact_inode
        ),
        "unsupported nofollow path chmod orphaned the private artifact"
    );
}

#[cfg(unix)]
#[test]
fn runtime_blocks_quarantine_entry_replacement_before_unlink() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let (_temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer retained across quarantine entry race");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    let original_inode = std::fs::symlink_metadata(&response)
        .expect("response metadata")
        .ino();
    let expected_relative = std::path::PathBuf::from("responses")
        .join(response.file_name().expect("response artifact file name"));
    let response_parent = response.parent().expect("response directory").to_path_buf();
    let hook_parent = response_parent.clone();

    crate::workspace::with_secure_remove_test_hook(
        move |boundary, relative| {
            if boundary == crate::workspace::SecureRemoveTestBoundary::QuarantineIdentityVerified
                && relative == expected_relative
            {
                let quarantine = cleanup_quarantine(&hook_parent);
                std::fs::set_permissions(&quarantine, std::fs::Permissions::from_mode(0o700))
                    .expect("owner reopens cleanup quarantine");
                std::fs::rename(quarantine.join("artifact"), quarantine.join("retained"))
                    .expect("retain opened quarantine artifact");
                std::fs::write(quarantine.join("artifact"), "replacement private artifact")
                    .expect("replace quarantine entry");
            }
        },
        || app.tick_receiver(),
    );

    assert!(
        cleanup_authority_exists(&app, first.job_id()),
        "quarantine entry replacement discarded cleanup authority"
    );
    let quarantine = cleanup_quarantine(&response_parent);
    assert!(
        std::fs::symlink_metadata(&quarantine)
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o777 == 0o700),
        "fail-closed quarantine was not owner-private and restart-openable"
    );
    std::fs::set_permissions(&quarantine, std::fs::Permissions::from_mode(0o700))
        .expect("inspect cleanup quarantine");
    assert!(
        std::fs::symlink_metadata(quarantine.join("retained"))
            .is_ok_and(|metadata| metadata.ino() == original_inode),
        "quarantine race lost the originally opened artifact"
    );
    assert!(
        quarantine.join("artifact").exists(),
        "cleanup unlinked a replacement quarantine entry"
    );
}

#[cfg(unix)]
#[test]
fn runtime_blocks_reappeared_original_after_artifact_unlink_interruption() {
    let (temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer removed before directory cleanup");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    let expected_relative = std::path::PathBuf::from("responses")
        .join(response.file_name().expect("response artifact file name"));
    let replacement_path = response.clone();

    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::workspace::with_secure_remove_test_hook(
            move |boundary, relative| {
                if boundary
                    == crate::workspace::SecureRemoveTestBoundary::QuarantineArtifactUnlinkedBeforeDirectoryRemoval
                    && relative == expected_relative
                {
                    std::fs::write(&replacement_path, "replacement private artifact")
                        .expect("reintroduce original cleanup name");
                    panic!("simulated interruption after quarantine artifact unlink");
                }
            },
            || app.tick_receiver(),
        );
    }));
    assert!(
        interrupted.is_err(),
        "cleanup interruption hook did not run"
    );
    assert!(cleanup_authority_exists(&app, first.job_id()));
    assert!(
        cleanup_quarantine(response.parent().expect("response directory"))
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with("-active")),
        "post-unlink quarantine did not retain its active phase"
    );
    drop(app);

    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(&temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    restarted
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    restarted.tick_receiver();

    assert!(
        cleanup_authority_exists(&restarted, first.job_id()),
        "empty quarantine retry discarded cleanup authority"
    );
    assert!(
        response.exists(),
        "empty quarantine retry deleted the replacement"
    );
}

#[cfg(unix)]
#[test]
fn runtime_blocks_reappeared_original_at_post_unlink_success_fence() {
    let (_temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer removed before final absence fence");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    let expected_relative = std::path::PathBuf::from("responses")
        .join(response.file_name().expect("response artifact file name"));
    let replacement_path = response.clone();

    crate::workspace::with_secure_remove_test_hook(
        move |boundary, relative| {
            if boundary
                == crate::workspace::SecureRemoveTestBoundary::QuarantineArtifactUnlinkedBeforeDirectoryRemoval
                && relative == expected_relative
            {
                std::fs::write(&replacement_path, "replacement private artifact")
                    .expect("reintroduce original cleanup name");
            }
        },
        || app.tick_receiver(),
    );

    assert!(
        cleanup_authority_exists(&app, first.job_id()),
        "post-unlink replacement discharged runtime cleanup authority"
    );
    assert!(
        response.exists(),
        "cleanup deleted the post-unlink replacement"
    );
    assert!(
        cleanup_quarantine(response.parent().expect("response directory"))
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with("-active")),
        "post-unlink replacement did not retain the active quarantine"
    );

    app.tick_receiver();

    assert!(
        cleanup_authority_exists(&app, first.job_id()),
        "blocked post-unlink replacement retry discarded cleanup authority"
    );
    assert!(response.exists(), "blocked retry deleted the replacement");
}

#[cfg(unix)]
#[test]
fn runtime_recovers_an_interruption_immediately_after_quarantine_promotion() {
    let (temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer retained after phase promotion");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    let expected_relative = std::path::PathBuf::from("responses")
        .join(response.file_name().expect("response artifact file name"));

    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::workspace::with_secure_remove_test_hook(
            move |boundary, relative| {
                if boundary
                    == crate::workspace::SecureRemoveTestBoundary::QuarantinePromotedBeforeArtifactVerification
                    && relative == expected_relative
                {
                    panic!("simulated interruption after quarantine promotion");
                }
            },
            || app.tick_receiver(),
        );
    }));
    assert!(
        interrupted.is_err(),
        "cleanup interruption hook did not run"
    );
    assert!(cleanup_authority_exists(&app, first.job_id()));
    assert!(
        cleanup_quarantine(response.parent().expect("response directory"))
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with("-active")),
        "promoted quarantine did not persist its active phase"
    );
    drop(app);

    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(&temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    restarted
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    restarted.tick_receiver();

    assert!(
        !cleanup_authority_exists(&restarted, first.job_id()),
        "promoted quarantine interruption stranded cleanup authority"
    );
    assert!(!response.exists());
}
