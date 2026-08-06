//! Integration: the `notify` watcher fires (calls its on-fire callback) shortly
//! after a file under the watched root changes. Uses a tiny debounce window and
//! a test callback; it never touches rclone, the lock, or B2. Robust to FS-event
//! latency via a bounded poll.
//!
//! macOS runs this through Brain's deterministic polling fallback; other
//! platforms exercise notify's recommended native backend.

use std::sync::mpsc;
use std::time::Duration;

use brain::sync::watch::spawn_watcher_with;

#[test]
fn watcher_fires_after_a_file_changes() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("personal");
    std::fs::create_dir_all(&root).unwrap();

    let (fired_tx, fired_rx) = mpsc::channel();
    let handle = spawn_watcher_with(&root, Duration::from_millis(200), move || {
        let _ = fired_tx.send(());
    })
    .expect("watcher starts");

    // Emit a short burst, then become completely quiet so the debounce window
    // can expire.
    for n in 1..=3 {
        std::fs::write(root.join(format!("note{n}.md")), b"hello").unwrap();
    }
    fired_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("watcher should fire after a change");

    drop(handle); // explicitly stops and joins this watcher worker
}

#[test]
fn stopping_one_workspace_watcher_leaves_the_peer_running() {
    let temporary = tempfile::tempdir().unwrap();
    let personal = temporary.path().join("personal");
    let family = temporary.path().join("family");
    std::fs::create_dir_all(&personal).unwrap();
    std::fs::create_dir_all(&family).unwrap();
    let (personal_tx, personal_rx) = mpsc::channel();
    let (family_tx, family_rx) = mpsc::channel();
    let personal_handle = spawn_watcher_with(&personal, Duration::from_millis(20), move || {
        let _ = personal_tx.send(());
    })
    .expect("personal watcher starts");
    let family_handle = spawn_watcher_with(&family, Duration::from_millis(20), move || {
        let _ = family_tx.send(());
    })
    .expect("family watcher starts");

    std::fs::write(personal.join("first.md"), b"personal").unwrap();
    std::fs::write(family.join("first.md"), b"family").unwrap();
    personal_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("personal watcher fires");
    family_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("family watcher fires");

    drop(personal_handle);
    std::fs::write(family.join("second.md"), b"family remains live").unwrap();
    family_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("family watcher survives personal shutdown");
    assert!(personal_rx.recv_timeout(Duration::from_millis(20)).is_err());
    drop(family_handle);
}
