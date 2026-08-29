#[cfg(unix)]
use std::os::unix::fs::symlink;

use super::receiver_durable_answer_cleanup::answer_fixture;
use super::receiver_durable_support::publish_valid_completion;
use super::*;

#[cfg(unix)]
fn quarantine_contains_inode(parent: &std::path::Path, inode: u64) -> bool {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    std::fs::read_dir(parent)
        .expect("cleanup parent entries")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".brain-cleanup-")
        })
        .any(|entry| {
            std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(0o700))
                .expect("inspect private cleanup quarantine");
            std::fs::symlink_metadata(entry.path().join("artifact"))
                .is_ok_and(|metadata| metadata.ino() == inode)
        })
}

#[cfg(unix)]
#[test]
fn runtime_answer_cleanup_rejects_symlinked_artifact_ancestors_and_retries_exact_paths() {
    let (temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer retained behind exact cleanup authority");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    assert!(
        response.exists(),
        "fixture did not retain the response artifact"
    );

    let instance = response
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .expect("response instance");
    let responses = app.context.workspace().paths().responses_dir();
    let observations = app.context.workspace().paths().receiver_observations_dir();
    std::fs::create_dir_all(&observations).expect("observation directory");
    let observation = observations.join(format!("{instance}.json"));
    let observation_lock = observation.with_extension("json.lock");
    std::fs::write(&observation, "private observation").expect("observation artifact");
    std::fs::write(&observation_lock, "private observation lock").expect("observation lock");

    let real_responses = responses.with_extension("real");
    let real_observations = observations.with_extension("real");
    std::fs::rename(&responses, &real_responses).expect("retain exact responses directory");
    std::fs::rename(&observations, &real_observations)
        .expect("retain exact observations directory");
    let outside = temporary.path().join("outside-cleanup");
    let outside_responses = outside.join("responses");
    let outside_observations = outside.join("receiver-observations");
    std::fs::create_dir_all(&outside_responses).expect("outside responses");
    std::fs::create_dir_all(&outside_observations).expect("outside observations");
    let outside_response = outside_responses.join(format!("{instance}.json"));
    let outside_observation = outside_observations.join(format!("{instance}.json"));
    let outside_observation_lock = outside_observation.with_extension("json.lock");
    for path in [
        &outside_response,
        &outside_observation,
        &outside_observation_lock,
    ] {
        std::fs::write(path, "outside private artifact").expect("outside artifact");
    }
    symlink(&outside_responses, &responses).expect("responses ancestor symlink");
    symlink(&outside_observations, &observations).expect("observations ancestor symlink");

    app.tick_receiver();

    assert!(
        outside_response.exists()
            && outside_observation.exists()
            && outside_observation_lock.exists(),
        "runtime cleanup deleted an outside artifact through a symlinked ancestor"
    );
    assert!(
        cleanup_authority_exists(&app, first.job_id()),
        "runtime cleanup discarded authority after rejecting a symlinked ancestor"
    );

    std::fs::remove_file(&responses).expect("remove responses ancestor symlink");
    std::fs::remove_file(&observations).expect("remove observations ancestor symlink");
    std::fs::rename(&real_responses, &responses).expect("restore exact responses directory");
    std::fs::rename(&real_observations, &observations)
        .expect("restore exact observations directory");

    app.tick_receiver();

    assert!(
        !cleanup_authority_exists(&app, first.job_id()),
        "exact restored cleanup authority did not finish"
    );
    assert!(
        !response.exists(),
        "exact response artifact survived cleanup"
    );
    assert!(
        !observation.exists(),
        "exact observation artifact survived cleanup"
    );
    assert!(
        !observation_lock.exists(),
        "exact observation lock survived cleanup"
    );
}

#[cfg(unix)]
#[test]
fn runtime_answer_cleanup_never_unlinks_a_replacement_after_quarantine_verification() {
    use std::cell::Cell;
    use std::os::unix::fs::MetadataExt as _;
    use std::rc::Rc;

    let (_temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer retained during exact cleanup race");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    let opened_inode = std::fs::symlink_metadata(&response)
        .expect("opened response metadata")
        .ino();
    let expected_relative = std::path::PathBuf::from("responses")
        .join(response.file_name().expect("response artifact file name"));
    let replacement_inode = Rc::new(Cell::new(None));
    let hook_inode = Rc::clone(&replacement_inode);
    let hook_response = response.clone();

    crate::workspace::with_secure_remove_test_hook(
        move |boundary, relative| {
            if boundary == crate::workspace::SecureRemoveTestBoundary::QuarantineIdentityVerified
                && relative == expected_relative
                && hook_inode.get().is_none()
            {
                std::fs::write(&hook_response, "replacement private artifact")
                    .expect("replace response artifact");
                hook_inode.set(Some(
                    std::fs::symlink_metadata(&hook_response)
                        .expect("replacement metadata")
                        .ino(),
                ));
            }
        },
        || app.tick_receiver(),
    );

    let replacement_inode = replacement_inode.get().expect("replacement race hook ran");
    assert!(
        cleanup_authority_exists(&app, first.job_id()),
        "runtime cleanup discarded authority after an exact-target mismatch"
    );
    assert!(
        std::fs::symlink_metadata(&response)
            .is_ok_and(|metadata| metadata.ino() == replacement_inode),
        "runtime cleanup unlinked the replacement at the original leaf"
    );
    assert!(
        quarantine_contains_inode(response.parent().expect("response directory"), opened_inode,),
        "runtime cleanup deleted the opened artifact before resolving the replacement"
    );
    app.tick_receiver();
    assert!(
        cleanup_authority_exists(&app, first.job_id()),
        "runtime cleanup retry discarded fail-closed mismatch authority"
    );
    assert!(
        quarantine_contains_inode(response.parent().expect("response directory"), opened_inode,),
        "runtime cleanup retry deleted the quarantined opened artifact"
    );
    assert!(
        std::fs::symlink_metadata(&response)
            .is_ok_and(|metadata| metadata.ino() == replacement_inode),
        "runtime cleanup retry unlinked the replacement at the original leaf"
    );
}

#[cfg(unix)]
#[test]
fn runtime_answer_cleanup_recovers_an_artifact_after_interruption_during_quarantine() {
    use std::os::unix::fs::MetadataExt as _;

    let (temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer retained across cleanup interruption");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    let opened_inode = std::fs::symlink_metadata(&response)
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
                    panic!("simulated cleanup interruption");
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
        !response.exists(),
        "interrupted rename left the original leaf"
    );
    assert!(
        quarantine_contains_inode(response.parent().expect("response directory"), opened_inode),
        "interrupted rename did not retain the private artifact"
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
        "recovered quarantine did not release runtime cleanup authority"
    );
    assert!(
        !quarantine_contains_inode(response.parent().expect("response directory"), opened_inode),
        "runtime retry orphaned the quarantined private artifact"
    );
}

#[cfg(unix)]
#[test]
fn runtime_answer_cleanup_blocks_reappeared_original_after_hardened_quarantine_interruption() {
    use std::os::unix::fs::MetadataExt as _;

    let (_temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer retained after hardened interruption");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    let opened_inode = std::fs::symlink_metadata(&response)
        .expect("response metadata")
        .ino();
    let expected_relative = std::path::PathBuf::from("responses")
        .join(response.file_name().expect("response artifact file name"));

    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::workspace::with_secure_remove_test_hook(
            move |boundary, relative| {
                if boundary
                    == crate::workspace::SecureRemoveTestBoundary::QuarantineIdentityVerified
                    && relative == expected_relative
                {
                    panic!("simulated hardened cleanup interruption");
                }
            },
            || app.tick_receiver(),
        );
    }));
    assert!(
        interrupted.is_err(),
        "hardened interruption hook did not run"
    );
    std::fs::write(&response, "replacement private artifact")
        .expect("reappear original response name");
    let replacement_inode = std::fs::symlink_metadata(&response)
        .expect("replacement metadata")
        .ino();

    app.tick_receiver();
    app.tick_receiver();

    assert!(
        cleanup_authority_exists(&app, first.job_id()),
        "reappeared original discarded runtime cleanup authority"
    );
    assert!(
        std::fs::symlink_metadata(&response)
            .is_ok_and(|metadata| metadata.ino() == replacement_inode),
        "retry unlinked the reappeared original"
    );
    assert!(
        quarantine_contains_inode(response.parent().expect("response directory"), opened_inode),
        "retry deleted the hardened quarantined artifact"
    );
}

#[cfg(unix)]
#[test]
fn runtime_answer_cleanup_persistently_blocks_a_replacement_after_rename_reports_missing() {
    use std::cell::Cell;
    use std::os::unix::fs::MetadataExt as _;
    use std::rc::Rc;

    let (_temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer retained during missing rename race");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    let expected_relative = std::path::PathBuf::from("responses")
        .join(response.file_name().expect("response artifact file name"));
    let replacement_inode = Rc::new(Cell::new(None));
    let hook_inode = Rc::clone(&replacement_inode);
    let hook_response = response.clone();

    crate::workspace::with_secure_remove_test_hook(
        move |boundary, relative| {
            if relative != expected_relative {
                return;
            }
            if boundary
                == crate::workspace::SecureRemoveTestBoundary::EntryIdentityVerifiedBeforeRename
            {
                std::fs::remove_file(&hook_response).expect("remove response before rename");
            } else if boundary
                == crate::workspace::SecureRemoveTestBoundary::RenameMissingBeforeAbsenceCheck
            {
                std::fs::write(&hook_response, "replacement private artifact")
                    .expect("replace response after missing rename");
                hook_inode.set(Some(
                    std::fs::symlink_metadata(&hook_response)
                        .expect("replacement metadata")
                        .ino(),
                ));
            }
        },
        || app.tick_receiver(),
    );

    let replacement_inode = replacement_inode.get().expect("replacement race hook ran");
    assert!(
        cleanup_authority_exists(&app, first.job_id()),
        "missing rename race discarded runtime cleanup authority"
    );
    app.tick_receiver();
    assert!(
        cleanup_authority_exists(&app, first.job_id()),
        "retry discarded fail-closed authority after missing rename"
    );
    assert!(
        std::fs::symlink_metadata(&response)
            .is_ok_and(|metadata| metadata.ino() == replacement_inode),
        "retry unlinked the replacement after missing rename"
    );
}

fn cleanup_authority_exists(app: &App, job_id: crate::state::ReceiverJobId) -> bool {
    rusqlite::Connection::open(app.context.state_db_path())
        .expect("open receiver state for cleanup authority")
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM receiver_answer_cleanups WHERE job_id = ?1)",
            [job_id.to_string()],
            |row| row.get(0),
        )
        .expect("answer cleanup authority")
}
