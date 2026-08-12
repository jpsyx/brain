use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use super::*;

#[test]
fn failed_setup_rollback_preserves_a_concurrent_success_and_live_lock_inode() {
    use std::os::unix::fs::MetadataExt as _;

    let fixture = Fixture::new();
    let context = fixture.context.clone();
    let home = fixture.home.clone();
    let provider_written = Arc::new(Barrier::new(2));
    let release_failure = Arc::new(Barrier::new(2));
    let thread_provider_written = Arc::clone(&provider_written);
    let thread_release_failure = Arc::clone(&release_failure);
    let lock = home.join(".codex/.hooks.json.transaction.lock");
    std::fs::write(&lock, b"live lock").unwrap();
    let lock_inode = std::fs::metadata(&lock).unwrap().ino();

    let failing = std::thread::spawn(move || {
        persist_plan_with_hook(&plan(), &context, &home, |step| {
            if step == CommitStep::Providers {
                thread_provider_written.wait();
                thread_release_failure.wait();
                anyhow::bail!("injected failure after concurrent success");
            }
            Ok(())
        })
    });
    provider_written.wait();
    crate::env::set(&fixture.context, "codex_cmd", "concurrent-codex").unwrap();
    release_failure.wait();

    assert!(failing.join().unwrap().is_err());
    assert_eq!(
        crate::env::get(&fixture.context, "codex_cmd").as_deref(),
        Some("concurrent-codex")
    );
    assert_eq!(std::fs::metadata(lock).unwrap().ino(), lock_inode);
}

#[test]
fn setup_serializes_identical_after_images_across_rollback_ownership() {
    let fixture = Fixture::new();
    let failing_context = fixture.context.clone();
    let failing_home = fixture.home.clone();
    let provider_written = Arc::new(Barrier::new(2));
    let release_failure = Arc::new(Barrier::new(2));
    let thread_provider_written = Arc::clone(&provider_written);
    let thread_release_failure = Arc::clone(&release_failure);
    let failing = std::thread::spawn(move || {
        persist_plan_with_hook(&plan(), &failing_context, &failing_home, |step| {
            if step == CommitStep::Providers {
                thread_provider_written.wait();
                thread_release_failure.wait();
            }
            anyhow::ensure!(
                step != CommitStep::Users,
                "injected failure after identical concurrent setup"
            );
            Ok(())
        })
    });
    provider_written.wait();

    let concurrent_context = fixture.context.clone();
    let concurrent_home = fixture.home.clone();
    let (concurrent_tx, concurrent_rx) = std::sync::mpsc::sync_channel(1);
    let concurrent = std::thread::spawn(move || {
        concurrent_tx
            .send(persist_plan_with_hook(
                &plan(),
                &concurrent_context,
                &concurrent_home,
                |_| Ok(()),
            ))
            .expect("report concurrent setup");
    });
    let early = concurrent_rx.recv_timeout(Duration::from_millis(250));
    let concurrent_was_blocked = matches!(&early, Err(std::sync::mpsc::RecvTimeoutError::Timeout));
    release_failure.wait();
    let failing_result = failing.join().expect("failing setup worker");
    let concurrent_result = match early {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => concurrent_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("serialized setup completion"),
        Err(error) => panic!("concurrent setup channel failed: {error}"),
    };
    concurrent.join().expect("concurrent setup worker");

    assert!(failing_result.is_err());
    assert!(
        concurrent_was_blocked,
        "concurrent setup crossed the active transaction"
    );
    concurrent_result.expect("serialized concurrent setup");
    assert_eq!(
        crate::env::get(&fixture.context, "resend_sending_api_key").as_deref(),
        Some("re_secret")
    );
    assert!(
        UsersStore::load(&fixture.context.workspace)
            .expect("users after serialized setup")
            .users
            .is_empty()
    );
}

#[test]
fn setup_lock_timeout_is_bounded_actionable_and_mutates_nothing() {
    let fixture = Fixture::new();
    let holder = std::sync::Mutex::new(Some(
        SetupTransactionLock::acquire(fixture.context.workspace.root()).unwrap(),
    ));
    let started = Instant::now();
    let deadline = started + Duration::from_millis(10);
    let current = std::sync::Mutex::new(started);
    let clock = || {
        *current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    let poll = |_| {
        holder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        *current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = deadline;
    };

    let error = SetupTransactionLock::acquire_until(
        fixture.context.workspace.root(),
        deadline,
        &clock,
        &poll,
    )
    .unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("receiver setup"), "{message}");
    assert!(message.contains("timed out"), "{message}");
    fixture.assert_restored();
}

#[test]
fn setup_lock_rejects_an_already_elapsed_deadline_even_when_free() {
    let fixture = Fixture::new();
    let deadline = Instant::now();

    let error = SetupTransactionLock::acquire_until(
        fixture.context.workspace.root(),
        deadline,
        &|| deadline,
        &|_| panic!("elapsed acquisition must not poll"),
    )
    .unwrap_err();

    assert!(error.to_string().contains("timed out"), "{error:#}");
    fixture.assert_restored();
}
