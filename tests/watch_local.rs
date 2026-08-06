//! Integration: the `notify` watcher fires (calls its on-fire callback) shortly
//! after a file under the watched root changes. Uses a tiny debounce window and
//! a test callback — never touches rclone, the lock, or B2. Robust to FS-event
//! latency via a bounded poll.
//!
//! macOS runs this through Brain's deterministic polling fallback; other
//! platforms exercise notify's recommended native backend.

use std::sync::mpsc;
use std::time::Duration;

use brain::sync::watch::spawn_watcher_with;

#[test]
fn watcher_fires_after_a_file_changes() {
    // Use a user-owned tree, matching production's `~/brain` ownership model.
    let root = std::env::var_os("HOME")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
        .join(format!("brain-watch-test-{}", std::process::id()));
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
    std::fs::remove_dir_all(&root).ok();
}
