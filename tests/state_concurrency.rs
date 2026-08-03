//! Verify the session state DB survives concurrent access from multiple
//! writers — the realistic load is N brain shells plus the SessionStart
//! hook (a separate process) all touching `brain_sessions`. WAL mode + the
//! configured busy_timeout should make this collision-free, and `claim` must
//! hand a given free session to exactly one shell.

// Test ergonomics: `let handles: Vec<_> = …collect()` deliberately forces
// every thread to spawn before we join (not a `needless_collect`); small loop
// indices cast to i32 for fake pids; per-test `const N` sits with its loop.
#![allow(
    clippy::needless_collect,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::items_after_statements
)]

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use brain::state::Db;

fn fresh_db() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("state.db");
    drop(Db::open_path(&path).unwrap());
    (tmp, path)
}

#[test]
fn concurrent_register_fresh_from_n_threads_do_not_clobber() {
    let (_tmp, path) = fresh_db();

    // Ten "tasks shells", each registers its own fresh session in parallel.
    // sqlite + WAL serialises writers but never errors out (within
    // busy_timeout). Every row must land.
    const SHELLS: usize = 10;
    let path = Arc::new(path);
    let handles: Vec<_> = (0..SHELLS)
        .map(|i| {
            let path = Arc::clone(&path);
            thread::spawn(move || {
                let db = Db::open_path(&path).expect("open per-thread");
                db.register_fresh(&format!("sess-{i}"), &format!("inst-{i}"), 1000 + i as i32)
                    .expect("register_fresh");
                // Then release so the row becomes resumable.
                db.release(&format!("inst-{i}")).expect("release");
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread join");
    }

    let db = Db::open_path(&path).unwrap();
    assert_eq!(
        db.free_sessions_by_recency().len(),
        SHELLS,
        "every registered+released session is resumable"
    );
}

#[test]
fn only_one_thread_wins_a_claim_on_a_shared_free_session() {
    // Reproduces two shells racing to resume the same free session: `claim`
    // is a conditional UPDATE, so exactly one must win.
    let (_tmp, path) = fresh_db();
    {
        let db = Db::open_path(&path).unwrap();
        db.register_fresh("shared", "seed", 1).unwrap();
        db.release("seed").unwrap(); // now free
    }

    const RACERS: usize = 8;
    let path = Arc::new(path);
    let handles: Vec<_> = (0..RACERS)
        .map(|i| {
            let path = Arc::clone(&path);
            thread::spawn(move || {
                let db = Db::open_path(&path).expect("open per-thread");
                db.claim("shared", &format!("inst-{i}"), 2000 + i as i32)
                    .expect("claim")
            })
        })
        .collect();

    let wins = handles
        .into_iter()
        .map(|h| h.join().expect("join"))
        .filter(|won| *won)
        .count();
    assert_eq!(wins, 1, "exactly one shell may lock a free session");
}
