use super::*;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

#[test]
fn disarmed_debouncer_never_fires() {
    let mut d = Debouncer::new(Duration::from_secs(3));
    assert!(!d.poll(Instant::now()));
    assert_eq!(d.time_until_fire(Instant::now()), None);
}

#[test]
fn fires_once_after_the_window_then_disarms() {
    let t0 = Instant::now();
    let mut d = Debouncer::new(Duration::from_secs(3));
    d.on_event(t0);
    assert!(!d.poll(t0 + Duration::from_secs(1)), "not yet quiet");
    assert!(d.poll(t0 + Duration::from_secs(3)), "fires at the window");
    assert!(
        !d.poll(t0 + Duration::from_secs(4)),
        "disarmed after firing"
    );
}

#[test]
fn a_burst_coalesces_into_a_single_fire() {
    let t0 = Instant::now();
    let mut d = Debouncer::new(Duration::from_secs(3));
    d.on_event(t0);
    d.on_event(t0 + Duration::from_secs(1)); // re-arms → deadline = t0+4s
    d.on_event(t0 + Duration::from_secs(2)); // re-arms → deadline = t0+5s
    assert!(
        !d.poll(t0 + Duration::from_secs(4)),
        "still within the extended window"
    );
    assert!(
        d.poll(t0 + Duration::from_secs(5)),
        "one fire once the burst settles"
    );
    assert!(!d.poll(t0 + Duration::from_secs(6)));
}

#[test]
fn time_until_fire_counts_down() {
    let t0 = Instant::now();
    let mut d = Debouncer::new(Duration::from_secs(3));
    d.on_event(t0);
    assert_eq!(d.time_until_fire(t0), Some(Duration::from_secs(3)));
    assert_eq!(
        d.time_until_fire(t0 + Duration::from_secs(3)),
        Some(Duration::ZERO)
    );
}

#[test]
fn excludes_vcs_cache_os_cruft_and_conflict_copies() {
    assert!(!is_watch_relevant(Path::new(".git/index")));
    assert!(!is_watch_relevant(Path::new("notes/.DS_Store")));
    assert!(!is_watch_relevant(Path::new(".cache/x")));
    assert!(!is_watch_relevant(Path::new(
        "notes/idea (conflict mac 2026-07-25).md"
    )));
}

#[test]
fn excludes_the_raw_rclone_conflict_marker() {
    // The marker rclone leaves before the friendly rename must not re-trigger
    // a sync (mirrors the bisync `*.__brainconflict__*` exclude).
    assert!(!is_watch_relevant(Path::new(
        "notes/idea.md.__brainconflict__"
    )));
}

/// A dependency install is thousands of writes the sync now excludes, so
/// triggering on them means a debounced sync that transfers nothing, once
/// per agent launch. It must mirror the exclude set, not lag behind it.
#[test]
fn a_dependency_tree_never_triggers_a_sync() {
    assert!(!is_watch_relevant(Path::new(
        ".opencode/node_modules/zod/index.js"
    )));
    assert!(!is_watch_relevant(Path::new(
        "projects/thing/node_modules/x/y.js"
    )));
    assert!(!is_watch_relevant(Path::new(".opencode/package-lock.json")));
    assert!(!is_watch_relevant(Path::new(".opencode/bun.lock")));
    assert!(!is_watch_relevant(Path::new(
        ".agents/skills/todo/scripts/__pycache__/csvlib.cpython-314.pyc"
    )));
    assert!(!is_watch_relevant(Path::new("scripts/session_hook.pyc")));
    // Brain's own bridge is content, so it still triggers.
    assert!(is_watch_relevant(Path::new(".opencode/plugins/brain.js")));
}

/// Mid-transaction is the worst possible moment to trigger a push: the
/// journal and its scratch are excluded from transfer, so a sync fired by
/// them can only transfer a half-applied group of live files.
#[test]
fn a_transaction_journal_or_lock_never_triggers_a_sync() {
    for path in [
        ".config/.brain-user-transaction.json",
        ".config/.brain-user-4213-17e9-0.staged",
        ".config/.brain-user-4213-17e9-0.backup",
        ".config/.brain-triage-habits-transaction.json",
        "tasks/.tasks.csv.brain-triage-9f2-0.staged",
        "tasks/.brain-task-schema-tasks.staged",
        ".config/.receiver-setup.transaction.lock",
    ] {
        assert!(!is_watch_relevant(Path::new(path)), "{path}");
    }
}

#[test]
fn ordinary_notes_and_csvs_are_relevant() {
    assert!(is_watch_relevant(Path::new("projects/x/note.md")));
    assert!(is_watch_relevant(Path::new("tasks/tasks.csv")));
}

#[test]
fn stopping_one_clock_driven_watcher_loop_does_not_stop_its_peer() {
    let now = Arc::new(Mutex::new(Instant::now()));
    let (personal_tx, personal_rx) = mpsc::channel();
    let (family_tx, family_rx) = mpsc::channel();
    let (fired_tx, fired_rx) = mpsc::channel();
    let personal_clock = Arc::clone(&now);
    let personal_fired = fired_tx.clone();
    let personal = std::thread::spawn(move || {
        run_watcher_loop(
            &personal_rx,
            Duration::from_secs(3),
            move || personal_fired.send("personal").unwrap(),
            move || *personal_clock.lock().unwrap(),
        );
    });
    let family_clock = Arc::clone(&now);
    let family = std::thread::spawn(move || {
        run_watcher_loop(
            &family_rx,
            Duration::from_secs(3),
            move || fired_tx.send("family").unwrap(),
            move || *family_clock.lock().unwrap(),
        );
    });

    personal_tx
        .send(WatchInput::Paths(vec![
            Path::new("personal/note.md").to_path_buf(),
        ]))
        .unwrap();
    family_tx
        .send(WatchInput::Paths(vec![
            Path::new("family/note.md").to_path_buf(),
        ]))
        .unwrap();
    for sender in [&personal_tx, &family_tx] {
        let (observed_tx, observed_rx) = mpsc::channel();
        sender.send(WatchInput::Observed(observed_tx)).unwrap();
        observed_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    *now.lock().unwrap() += Duration::from_secs(3);
    personal_tx.send(WatchInput::Poll).unwrap();
    family_tx.send(WatchInput::Poll).unwrap();
    let first = fired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = fired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_ne!(first, second);

    personal_tx.send(WatchInput::Stop).unwrap();
    personal.join().unwrap();
    family_tx
        .send(WatchInput::Paths(vec![
            Path::new("family/second.md").to_path_buf(),
        ]))
        .unwrap();
    let (observed_tx, observed_rx) = mpsc::channel();
    family_tx.send(WatchInput::Observed(observed_tx)).unwrap();
    observed_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    *now.lock().unwrap() += Duration::from_secs(3);
    family_tx.send(WatchInput::Poll).unwrap();

    assert_eq!(
        fired_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        "family"
    );
    family_tx.send(WatchInput::Stop).unwrap();
    family.join().unwrap();
}
