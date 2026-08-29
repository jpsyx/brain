#![cfg(unix)]

use std::{
    ffi::OsStr,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};

use super::receiver_durable_answer_cleanup::answer_fixture;
use super::receiver_durable_cleanup_security::cleanup_authority_exists;
use super::receiver_durable_support::publish_valid_completion;
use super::*;

const CHILD_CASE: &str = "BRAIN_RESTRICTIVE_UMASK_CLEANUP_CHILD";

fn spawn_isolated_case(case: &str, test_name: &str) {
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args([test_name, "--exact", "--nocapture"])
        .env(CHILD_CASE, case)
        .output()
        .expect("spawn restrictive umask test child");
    assert!(
        output.status.success(),
        "restrictive umask child failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn quarantine_entries(parent: &Path) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => panic!("cleanup parent entries: {error}"),
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(".brain-cleanup-") && !name.starts_with(".brain-cleanup-blocked-")
        })
        .map(|entry| entry.path())
        .collect()
}

fn quarantines_under(parents: &[PathBuf]) -> Vec<PathBuf> {
    parents
        .iter()
        .flat_map(|parent| quarantine_entries(parent))
        .collect()
}

fn setup_restricted_cleanup() -> (
    tempfile::TempDir,
    App,
    crate::state::ReceiverJobId,
    PathBuf,
    Vec<PathBuf>,
) {
    let (temporary, mut app, _db, first, _second, _transport) = answer_fixture();
    let response = publish_valid_completion(&app, "answer retained across restrictive umask");
    app.receiver
        .inject_cleanup_failure(crate::tui::receiver::ReceiverCleanupBoundary::Artifacts);
    app.tick_receiver();
    let response_parent = response
        .parent()
        .expect("response parent directory")
        .to_path_buf();
    let observation_parent = app.context.workspace().paths().receiver_observations_dir();
    (
        temporary,
        app,
        first.job_id(),
        response,
        vec![response_parent, observation_parent],
    )
}

fn run_with_restrictive_umask<T>(operation: impl FnOnce() -> T) -> T {
    let prior = nix::sys::stat::umask(nix::sys::stat::Mode::from_bits_truncate(0o777));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
    nix::sys::stat::umask(prior);
    match result {
        Ok(result) => result,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[test]
fn restrictive_umask_normal_cleanup_remains_portable() {
    let test_name = "tui::app_brain::tests::receiver_durable_cleanup_umask::restrictive_umask_normal_cleanup_remains_portable";
    if std::env::var_os(CHILD_CASE).as_deref() != Some(OsStr::new("normal")) {
        spawn_isolated_case("normal", test_name);
        return;
    }

    let (_temporary, mut app, job_id, response, parents) = setup_restricted_cleanup();
    let observed_mode = Rc::new(std::cell::Cell::new(None));
    let hook_mode = Rc::clone(&observed_mode);
    let hook_parents = parents.clone();
    run_with_restrictive_umask(|| {
        crate::workspace::with_secure_remove_test_hook(
            move |boundary, _relative| {
                if boundary
                    == crate::workspace::SecureRemoveTestBoundary::QuarantineCreatedBeforeOpen
                    && hook_mode.get().is_none()
                {
                    let quarantine = quarantines_under(&hook_parents);
                    let mode = std::fs::symlink_metadata(&quarantine[0])
                        .expect("new quarantine metadata")
                        .permissions()
                        .mode()
                        & 0o777;
                    hook_mode.set(Some(mode));
                }
            },
            || crate::workspace::with_unsupported_recovery_nofollow_chmod(|| app.tick_receiver()),
        );
    });

    assert_eq!(observed_mode.get(), Some(0), "umask did not restrict mkdir");
    assert!(
        !cleanup_authority_exists(&app, job_id),
        "restrictive umask stranded normal cleanup authority"
    );
    assert!(
        !response.exists(),
        "restrictive umask retained the response"
    );
    assert!(quarantines_under(&parents).is_empty());
}

#[test]
fn restrictive_umask_interrupted_creation_recovers_portably() {
    let test_name = "tui::app_brain::tests::receiver_durable_cleanup_umask::restrictive_umask_interrupted_creation_recovers_portably";
    if std::env::var_os(CHILD_CASE).as_deref() != Some(OsStr::new("interrupted")) {
        spawn_isolated_case("interrupted", test_name);
        return;
    }

    let (temporary, mut app, job_id, response, parents) = setup_restricted_cleanup();
    let interrupted = run_with_restrictive_umask(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::workspace::with_secure_remove_test_hook(
                move |boundary, _relative| {
                    if boundary
                        == crate::workspace::SecureRemoveTestBoundary::QuarantineCreatedBeforeOpen
                    {
                        panic!("simulated interruption after restrictive mkdir");
                    }
                },
                || app.tick_receiver(),
            );
        }))
    });
    assert!(
        interrupted.is_err(),
        "cleanup interruption hook did not run"
    );
    let quarantines = quarantines_under(&parents);
    assert_eq!(quarantines.len(), 1, "interruption quarantine count");
    assert_eq!(
        std::fs::symlink_metadata(&quarantines[0])
            .expect("interrupted quarantine metadata")
            .permissions()
            .mode()
            & 0o777,
        0,
        "interrupted quarantine was not created under restrictive umask"
    );
    assert!(cleanup_authority_exists(&app, job_id));
    drop(app);

    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(&temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    restarted
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    crate::workspace::with_unsupported_recovery_nofollow_chmod(|| restarted.tick_receiver());

    assert!(
        !cleanup_authority_exists(&restarted, job_id),
        "restrictive umask interruption stranded cleanup authority"
    );
    assert!(
        !response.exists(),
        "recovery retained the response artifact"
    );
    assert!(quarantines_under(&parents).is_empty());
}

#[test]
fn ordinary_umask_interrupted_creation_recovers_portably() {
    let (temporary, mut app, job_id, response, parents) = setup_restricted_cleanup();
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::workspace::with_secure_remove_test_hook(
            move |boundary, _relative| {
                if boundary
                    == crate::workspace::SecureRemoveTestBoundary::QuarantineCreatedBeforeOpen
                {
                    panic!("simulated interruption after ordinary mkdir");
                }
            },
            || app.tick_receiver(),
        );
    }));
    assert!(
        interrupted.is_err(),
        "cleanup interruption hook did not run"
    );
    let quarantines = quarantines_under(&parents);
    assert_eq!(quarantines.len(), 1, "interruption quarantine count");
    assert_eq!(
        std::fs::symlink_metadata(&quarantines[0])
            .expect("interrupted quarantine metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700,
        "ordinary quarantine did not retain requested owner mode"
    );
    assert!(cleanup_authority_exists(&app, job_id));
    drop(app);

    let cli = Cli::parse_from(["tasks"]);
    let mut restarted = test_app(&temporary, &cli, AgentKind::Claude);
    restarted.receiver.record_intent(true);
    restarted
        .brain
        .replace_receiver_transport(TransportRecording::default().transport());
    crate::workspace::with_unsupported_recovery_nofollow_chmod(|| restarted.tick_receiver());

    assert!(
        !cleanup_authority_exists(&restarted, job_id),
        "ordinary umask interruption stranded cleanup authority"
    );
    assert!(
        !response.exists(),
        "ordinary umask recovery retained the response artifact"
    );
    assert!(quarantines_under(&parents).is_empty());
}
