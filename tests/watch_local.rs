//! Integration: the `notify` watcher fires (calls its on-fire callback) shortly
//! after a file under the watched root changes. Uses a tiny debounce window and
//! a test callback — never touches rclone, the lock, or B2. Robust to FS-event
//! latency via a bounded poll.
//!
//! macOS runs this through Brain's deterministic polling fallback; other
//! platforms exercise notify's recommended native backend.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use brain::sync::watch::spawn_watcher_with;

#[test]
fn watcher_fires_after_a_file_changes() {
    // Use a user-owned tree, matching production's `~/brain` ownership model.
    let root = std::env::var_os("HOME")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
        .join(format!("brain-watch-test-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();

    let fires = Arc::new(AtomicUsize::new(0));
    let f = fires.clone();
    let handle = spawn_watcher_with(&root, Duration::from_millis(200), move || {
        f.fetch_add(1, Ordering::SeqCst);
    })
    .expect("watcher starts");

    // Give the watcher a moment to register the recursive watch.
    std::thread::sleep(Duration::from_millis(400));

    // Emit a short burst, then become completely quiet so the debounce window
    // can expire.
    for n in 1..=3 {
        std::fs::write(root.join(format!("note{n}.md")), b"hello").unwrap();
        std::thread::sleep(Duration::from_millis(50));
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while fires.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        fires.load(Ordering::SeqCst) >= 1,
        "watcher should fire after a change"
    );

    drop(handle); // stops the watcher thread without blocking teardown
    std::fs::remove_dir_all(&root).ok();
}
