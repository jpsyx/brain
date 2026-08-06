use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use super::*;

#[test]
fn replacement_between_observation_and_reap_or_adoption_is_preserved() {
    let temporary = tempfile::tempdir().expect("temporary server directory");
    let paths = ServerPaths::from_directory(temporary.path().join("server"));
    fs::create_dir_all(paths.directory()).expect("create server directory");
    let stale = record(999_999, "57b162df-983a-45c3-ac7e-bad94eb27a99");
    let replacement = record(std::process::id(), "91a0cfc2-7427-49d5-a2f1-258f985cd7e5");

    create_lock(&paths, stale).expect("create stale owner");
    let observed = read_lock(&paths).expect("observe stale owner");
    replace_at_barrier(&paths, replacement);

    assert!(!remove_lock_if_observed(&paths, observed).expect("conditional reap"));
    assert!(
        !transfer_lock_if_observed(
            &paths,
            observed,
            record(std::process::id(), "00000000-0000-0000-0000-000000000001",)
        )
        .expect("conditional adoption")
    );
    assert_eq!(read_lock(&paths), Some(replacement));
}

#[test]
fn exact_cleanup_recheck_propagates_token_read_error() {
    let temporary = tempfile::tempdir().expect("temporary server directory");
    let paths = ServerPaths::from_directory(temporary.path().join("server"));
    fs::create_dir_all(paths.directory()).expect("create server directory");
    fs::create_dir(paths.election_lock()).expect("create unreadable token directory");
    let observed = record(std::process::id(), "57b162df-983a-45c3-ac7e-bad94eb27a99");

    let cleanup = remove_lock_if_observed(&paths, observed);

    assert!(cleanup.is_err());
}

#[test]
fn exact_cleanup_recheck_propagates_malformed_token_error() {
    let temporary = tempfile::tempdir().expect("temporary server directory");
    let paths = ServerPaths::from_directory(temporary.path().join("server"));
    fs::create_dir_all(paths.directory()).expect("create server directory");
    fs::write(paths.election_lock(), b"not-json").expect("write malformed token");
    let observed = record(std::process::id(), "57b162df-983a-45c3-ac7e-bad94eb27a99");

    let cleanup = remove_lock_if_observed(&paths, observed);

    assert!(cleanup.is_err());
}

#[test]
fn stale_reap_excludes_a_contender_until_the_observed_owner_is_removed() {
    let temporary = tempfile::tempdir().expect("temporary server directory");
    let paths = ServerPaths::from_directory(temporary.path().join("server"));
    fs::create_dir_all(paths.directory()).expect("create server directory");
    let stale = record(999_999, "57b162df-983a-45c3-ac7e-bad94eb27a99");
    let contender =
        ServerGeneration::parse("91a0cfc2-7427-49d5-a2f1-258f985cd7e5").expect("valid generation");
    create_lock(&paths, stale).expect("create stale owner");
    let reap_started = Arc::new(Barrier::new(2));
    let finish_reap = Arc::new(Barrier::new(2));
    let thread_paths = paths.clone();
    let thread_started = Arc::clone(&reap_started);
    let thread_finish = Arc::clone(&finish_reap);
    let reaper = std::thread::spawn(move || {
        let _mutex = ElectionMutex::acquire(&thread_paths).expect("lock election mutation");
        let observed = read_lock(&thread_paths).expect("observe stale owner");
        thread_started.wait();
        thread_finish.wait();
        assert!(remove_lock_if_observed(&thread_paths, observed).expect("conditional reap"));
    });
    reap_started.wait();

    assert!(
        ElectionGuard::try_acquire(&paths, contender)
            .expect("contending election")
            .is_none()
    );

    finish_reap.wait();
    reaper.join().expect("reaper thread");
    assert!(
        ElectionGuard::try_acquire(&paths, contender)
            .expect("election after reap")
            .is_some()
    );
}

#[test]
fn child_adoption_excludes_contenders_until_transfer_completes() {
    let temporary = tempfile::tempdir().expect("temporary server directory");
    let paths = ServerPaths::from_directory(temporary.path().join("server"));
    let generation =
        ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99").expect("valid generation");
    let contender =
        ServerGeneration::parse("91a0cfc2-7427-49d5-a2f1-258f985cd7e5").expect("valid generation");
    let parent = ElectionGuard::try_acquire(&paths, generation)
        .expect("parent election")
        .expect("parent owns election");
    let handoff = parent.handoff();
    let adoption_complete = Arc::new(Barrier::new(2));
    let release_child = Arc::new(Barrier::new(2));
    let thread_paths = paths.clone();
    let thread_adopted = Arc::clone(&adoption_complete);
    let thread_release = Arc::clone(&release_child);
    let child = std::thread::spawn(move || {
        let guard = ElectionGuard::adopt_for_pid(&thread_paths, generation, 999_999)
            .expect("child adoption");
        thread_adopted.wait();
        thread_release.wait();
        drop(guard);
    });
    adoption_complete.wait();
    handoff.cleanup().expect("finish parent handoff");
    assert!(validate_election_token(&paths, generation).is_ok());

    assert!(
        ElectionGuard::try_acquire(&paths, contender)
            .expect("contending election")
            .is_none()
    );

    release_child.wait();
    child.join().expect("child adoption thread");
    assert!(
        ElectionGuard::try_acquire(&paths, contender)
            .expect("election after adoption")
            .is_some()
    );
}

#[test]
fn parent_handoff_cleans_its_exact_token_when_child_is_lost_before_adoption() {
    let temporary = tempfile::tempdir().expect("temporary server directory");
    let paths = ServerPaths::from_directory(temporary.path().join("server"));
    let generation =
        ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99").expect("valid generation");
    let parent = ElectionGuard::try_acquire(&paths, generation)
        .expect("parent election")
        .expect("parent owns election");
    let child_ready = Arc::new(Barrier::new(2));
    let lose_child = Arc::new(Barrier::new(2));
    let thread_ready = Arc::clone(&child_ready);
    let thread_loss = Arc::clone(&lose_child);
    let child = std::thread::spawn(move || {
        thread_ready.wait();
        thread_loss.wait();
    });
    let handoff = parent.handoff();
    child_ready.wait();

    lose_child.wait();
    child.join().expect("pre-adoption child");
    handoff.cleanup().expect("clean parent handoff");

    assert!(!paths.election_lock().exists());
}

#[test]
fn parent_handoff_cleanup_survives_mutex_contention_after_child_loss() {
    let temporary = tempfile::tempdir().expect("temporary server directory");
    let paths = ServerPaths::from_directory(temporary.path().join("server"));
    let generation =
        ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99").expect("valid generation");
    let parent = ElectionGuard::try_acquire(&paths, generation)
        .expect("parent election")
        .expect("parent owns election");
    let handoff = parent.handoff();
    let mutex_held = Arc::new(Barrier::new(2));
    let release_mutex = Arc::new(Barrier::new(2));
    let holder_paths = paths.clone();
    let holder_ready = Arc::clone(&mutex_held);
    let holder_release = Arc::clone(&release_mutex);
    let holder = std::thread::spawn(move || {
        let _mutex = ElectionMutex::acquire(&holder_paths).expect("occupy election mutex");
        holder_ready.wait();
        holder_release.wait();
    });
    mutex_held.wait();
    let cleanup_started = Arc::new(Barrier::new(2));
    let cleanup_ready = Arc::clone(&cleanup_started);
    let (cleanup_finished_tx, cleanup_finished_rx) = mpsc::channel();
    let cleanup = std::thread::spawn(move || {
        cleanup_ready.wait();
        handoff.cleanup().expect("clean parent handoff");
        cleanup_finished_tx
            .send(())
            .expect("report handoff cleanup completion");
    });
    cleanup_started.wait();

    let finished_while_contended = cleanup_finished_rx
        .recv_timeout(Duration::from_millis(250))
        .is_ok();
    release_mutex.wait();
    holder.join().expect("mutex holder thread");
    if !finished_while_contended {
        cleanup_finished_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("handoff cleanup after contention");
    }
    cleanup.join().expect("handoff cleanup thread");

    assert!(!paths.election_lock().exists());
}

#[test]
fn parent_handoff_cleanup_propagates_token_read_error() {
    let temporary = tempfile::tempdir().expect("temporary server directory");
    let paths = ServerPaths::from_directory(temporary.path().join("server"));
    let generation =
        ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99").expect("valid generation");
    let parent = ElectionGuard::try_acquire(&paths, generation)
        .expect("parent election")
        .expect("parent owns election");
    let handoff = parent.handoff();
    let parent_record = handoff.record;
    fs::remove_file(paths.election_lock()).expect("remove parent token");
    fs::create_dir(paths.election_lock()).expect("replace token with unreadable directory");

    let cleanup = handoff.cleanup();

    assert!(cleanup.is_err());
    assert!(paths.election_lock().is_dir());
    fs::remove_dir(paths.election_lock()).expect("remove unreadable directory");
    create_lock(&paths, parent_record).expect("restore parent token");
    handoff.cleanup().expect("retry parent cleanup");
    assert!(!paths.election_lock().exists());
}

#[test]
fn parent_handoff_cleanup_propagates_malformed_token_error() {
    let temporary = tempfile::tempdir().expect("temporary server directory");
    let paths = ServerPaths::from_directory(temporary.path().join("server"));
    let generation =
        ServerGeneration::parse("57b162df-983a-45c3-ac7e-bad94eb27a99").expect("valid generation");
    let parent = ElectionGuard::try_acquire(&paths, generation)
        .expect("parent election")
        .expect("parent owns election");
    let handoff = parent.handoff();
    let parent_record = handoff.record;
    fs::write(paths.election_lock(), b"not-json").expect("write malformed token");

    let cleanup = handoff.cleanup();

    assert!(cleanup.is_err());
    assert_eq!(
        fs::read(paths.election_lock()).expect("read malformed token"),
        b"not-json"
    );
    write_lock(&paths, parent_record).expect("restore parent token");
    handoff.cleanup().expect("retry parent cleanup");
    assert!(!paths.election_lock().exists());
}

fn replace_at_barrier(paths: &ServerPaths, replacement: ElectionRecord) {
    let before_replace = Arc::new(Barrier::new(2));
    let after_replace = Arc::new(Barrier::new(2));
    let thread_paths = paths.clone();
    let thread_before = Arc::clone(&before_replace);
    let thread_after = Arc::clone(&after_replace);
    let replacement_thread = std::thread::spawn(move || {
        thread_before.wait();
        fs::remove_file(thread_paths.election_lock()).expect("remove observed owner");
        create_lock(&thread_paths, replacement).expect("create live replacement");
        thread_after.wait();
    });
    before_replace.wait();
    after_replace.wait();
    replacement_thread.join().expect("replacement thread");
}

fn record(pid: u32, generation: &str) -> ElectionRecord {
    ElectionRecord {
        pid,
        generation: ServerGeneration::parse(generation).expect("valid generation"),
    }
}
