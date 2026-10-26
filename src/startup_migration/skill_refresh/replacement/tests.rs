use super::*;
use std::{cell::RefCell, os::unix::fs::PermissionsExt as _};

thread_local! { static BEFORE_REPLACE: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None); }

pub(super) fn before_replace() {
    BEFORE_REPLACE.with(|hook| {
        if let Some(hook) = hook.take() {
            hook();
        }
    });
}

fn executable(path: &std::path::Path, version: &str) {
    fs::write(path, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'brain {version}'; else echo '--workspace -w sync'; fi\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn contender_at_finalization_boundary_cannot_reconcile_before_atomic_replacement() {
    let temporary = tempfile::tempdir().unwrap();
    let installed = temporary.path().join("brain");
    let candidate = temporary.path().join("target");
    executable(&installed, env!("CARGO_PKG_VERSION"));
    executable(&candidate, "0.101.0");
    let args = RefreshWorkspaceSkillsArgs {
        target_binary: candidate.clone(),
        target_version: "0.101.0".into(),
        phase: RefreshPhase::Apply,
        expected_target_sha256: Some(digest(&candidate).unwrap()),
        from_version: Some(env!("CARGO_PKG_VERSION").into()),
        replace_binary: Some(installed.clone()),
        expected_installed_sha256: Some(digest(&installed).unwrap()),
    };
    let target = Target::validate(&args).unwrap();
    let ownership =
        crate::startup_migration::invocation::Guard::exclusive_for_test(temporary.path()).unwrap();
    let replacement = Replacement::prepare(&args, &target).unwrap().unwrap();
    let lock = temporary.path().join(".cache/brain/invocations.lock");
    BEFORE_REPLACE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            let contender = fs::File::open(lock).unwrap();
            assert_eq!(
                fs2::FileExt::try_lock_shared(&contender)
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::WouldBlock
            );
        }));
    });
    replacement.publish(&target, ownership).unwrap();
    assert_eq!(
        digest(&installed).unwrap(),
        args.expected_target_sha256.unwrap()
    );
    let contender = fs::File::open(temporary.path().join(".cache/brain/invocations.lock")).unwrap();
    fs2::FileExt::try_lock_shared(&contender).unwrap();
}

#[test]
fn interruption_before_replacement_keeps_installed_bytes_and_revalidates_candidate_on_retry() {
    let temporary = tempfile::tempdir().unwrap();
    let installed = temporary.path().join("brain");
    let candidate = temporary.path().join("target");
    executable(&installed, env!("CARGO_PKG_VERSION"));
    executable(&candidate, "0.101.0");
    let before = fs::read(&installed).unwrap();
    let staged = fs::read(&candidate).unwrap();
    let args = RefreshWorkspaceSkillsArgs {
        target_binary: candidate.clone(),
        target_version: "0.101.0".into(),
        phase: RefreshPhase::Apply,
        expected_target_sha256: Some(digest(&candidate).unwrap()),
        from_version: Some(env!("CARGO_PKG_VERSION").into()),
        replace_binary: Some(installed.clone()),
        expected_installed_sha256: Some(digest(&installed).unwrap()),
    };
    let target = Target::validate(&args).unwrap();
    let ownership =
        crate::startup_migration::invocation::Guard::exclusive_for_test(temporary.path()).unwrap();
    let replacement = Replacement::prepare(&args, &target).unwrap().unwrap();
    let changed = candidate.clone();
    BEFORE_REPLACE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            fs::write(changed, "changed candidate").unwrap();
        }));
    });
    assert!(replacement.publish(&target, ownership).is_err());
    assert_eq!(fs::read(&installed).unwrap(), before);
    fs::write(&candidate, staged).unwrap();
    let ownership =
        crate::startup_migration::invocation::Guard::exclusive_for_test(temporary.path()).unwrap();
    Replacement::prepare(&args, &target)
        .unwrap()
        .unwrap()
        .publish(&target, ownership)
        .unwrap();
    assert_eq!(
        digest(&installed).unwrap(),
        args.expected_target_sha256.unwrap()
    );
}
