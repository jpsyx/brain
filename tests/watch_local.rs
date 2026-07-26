//! Integration: the `notify` watcher fires (calls its on-fire callback) shortly
//! after a file under the watched root changes. Uses a tiny debounce window and
//! a test callback — never touches rclone, the lock, or B2. Robust to FS-event
//! latency via a bounded poll.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use brain::sync::watch::spawn_watcher_with;

#[test]
fn watcher_fires_after_a_file_changes() {
    let root = std::env::temp_dir().join(format!("brain-watch-it-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();

    let fires = Arc::new(AtomicUsize::new(0));
    let f = fires.clone();
    let handle = spawn_watcher_with(&root, Duration::from_millis(200), move || {
        f.fetch_add(1, Ordering::SeqCst);
    })
    .expect("watcher starts");

    // Give the watcher a moment to register the recursive watch.
    std::thread::sleep(Duration::from_millis(400));

    // Emit a fresh change each iteration until the debounced fire lands. macOS
    // FSEvents can drop or lag the very first events on a cold watch, so a single
    // write is flaky; re-touching (distinct filenames) until it fires — or a
    // generous deadline elapses — keeps the test exercising the real fire path
    // without depending on the first event ever arriving. The sleep exceeds the
    // 200ms debounce window so each write can settle and fire.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut n = 0;
    while fires.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        n += 1;
        std::fs::write(root.join(format!("note{n}.md")), b"hello").unwrap();
        std::thread::sleep(Duration::from_millis(300));
    }
    assert!(fires.load(Ordering::SeqCst) >= 1, "watcher should fire after a change");

    drop(handle); // stops the watcher thread without blocking teardown
    std::fs::remove_dir_all(&root).ok();
}
