# Brain Sync C4 — auto-sync triggers (start / exit / watcher) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make sync **automatic**. Sync on the persistent shell's start (background) and exit (detached fire-and-forget), plus a debounced `notify` filesystem watcher over the brain root — all gated on `sync.on_start`/`on_exit`/`watch` (default-on when configured). Every trigger reuses `sync_once`, coalesces through a new machine-wide advisory lock, never blocks/slows the shell, and stays decoupled from the brain HTTP server.

**Architecture:** A small, pure-first trigger layer added to `src/sync/`. The **pure** cores — `lock::is_stale`, the `watch::Debouncer` state machine, `watch::is_watch_relevant`, `SyncConfig::debounce` — carry the decisions and the tests. The **thin** shells — `lock::try_acquire`/`Guard` (atomic lockfile + heartbeat), `watch::spawn_watcher` (the `notify` shell), `trigger::{run_locked_sync, sync_in_background, spawn_detached_sync}` — do IO/threads/`Command` over the tested cores. One TUI seam in `run_tui` wires the three triggers. The lock also closes a pre-existing race: two concurrent `brain sync` invocations.

**Tech Stack:** Rust, `anyhow`, `serde`/`serde_json`, `chrono` (already a dep), **`notify` 6 (new dependency)** for OS-native FS events, `std::thread` + `std::sync::mpsc`, `std::process::Command` with `process_group(0)` for detach (mirrors `src/server/lifecycle.rs`, no `unsafe`). `cargo test --release` + `cargo clippy --release --all-targets`.

---

## Scope

Phase **C4** of the [brain-sync spec](../specs/2026-07-24-brain-sync-design.md), detailed in the [C4 spec](../specs/2026-07-25-brain-sync-c4-triggers.md). Builds on **C2** (transport, journal, `sync_once`, `run_sync`) and **C3** (CSV merge — the CSVs sync through the same `sync_once`, so the watcher covers them for free), both merged to `main`. **In scope:** the sync lock, the pure `Debouncer` + watch predicate, the `notify` watcher shell, the start/exit/background triggers, the `debounce_ms` config, the `run_tui` wiring, and the `status` debounce line. **Out of scope:** the `/second-brain sync` + `resolve-conflicts` skill rows (C5); an idle-pull timer and a standalone always-on daemon (deferred, spec §11).

## File Structure

| File | Responsibility | Pure? |
| --- | --- | --- |
| `src/sync/lock.rs` (new) | Machine-wide advisory lock at `~/.cache/brain/sync/sync.lock`: pure `is_stale`; atomic `try_acquire` → `Guard` (heartbeat while held, reap-stale, release-on-drop) | pure decision + thin IO |
| `src/sync/watch.rs` (new) | Pure `Debouncer` (3s quiescence state machine) + `is_watch_relevant` predicate; the `notify` shell `spawn_watcher` + `WatcherHandle` | pure core + thin `notify` shell |
| `src/sync/trigger.rs` (new) | `run_locked_sync` (lock → `sync_once`), `sync_in_background` (spawn a thread), `spawn_detached_sync` (detached `brain sync` child for exit) | thin (over tested cores) |
| `src/sync/config.rs` (edit) | `debounce_ms` field (default 3000) + `debounce() -> Duration` | pure |
| `src/sync/command.rs` (edit) | extend `format_triggers` with the debounce window | pure |
| `src/sync/mod.rs` (edit) | `pub mod lock; pub mod watch; pub mod trigger;` | — |
| `src/main.rs` (edit) | wrap the manual `run_sync` in the sync lock | — |
| `src/tui/event_loop/setup.rs` (edit) | the `run_tui` seam: `on_start` background sync, hold the watcher handle, `on_exit` detached spawn | — |
| `Cargo.toml` (edit) | add `notify = "6"` | — |
| `tests/watch_local.rs` (new) | one FS-event integration test (touch → callback fires), no B2 | — |
| `docs/*`, `AGENTS.md` (edit) | per spec §12 | — |

---

## C4.1 — The sync lock (`lock.rs`) + wire the manual path

### Task 1: `lock.rs` — the machine-wide advisory sync lock

**Files:** Create `src/sync/lock.rs`; edit `src/sync/mod.rs`; edit `src/main.rs`.

- [ ] **Step 1: Write the failing test**

Create `src/sync/lock.rs`:

```rust
//! A machine-wide advisory lock so only one sync runs at a time. All sync entry
//! points (manual `brain sync`, the start/exit hooks, the watcher) acquire it;
//! whoever can't skips (best-effort, never blocks). The lockfile at
//! `~/.cache/brain/sync/sync.lock` holds the owning PID; a crash leaves a stale
//! file the next acquire reaps via PID-liveness (or a generous age backstop).

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Age backstop behind the PID-liveness reap: a lock older than this is treated
/// as stale even if some unrelated process now holds its PID.
const STALE_AGE: Duration = Duration::from_secs(600);

/// `~/.cache/brain/sync/sync.lock` — beside the journal (machine-local cache).
#[must_use]
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("HOME").map_or_else(
        || PathBuf::from("."),
        |h| PathBuf::from(h).join(".cache").join("brain").join("sync"),
    );
    base.join("sync.lock")
}

/// Pure staleness decision: a lock is stale if its owner is gone or it is older
/// than the age backstop.
#[must_use]
pub fn is_stale(owner_alive: bool, age: Duration, cap: Duration) -> bool {
    !owner_alive || age >= cap
}

/// Held lock; removes the lockfile on drop.
pub struct Guard {
    path: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Try to acquire the lock without blocking. `Some(Guard)` when we took it (no
/// live lock existed, or a stale one was reaped); `None` when a live sync holds
/// it — the caller should skip. Atomic via `create_new` (O_EXCL).
#[must_use]
pub fn try_acquire(path: &Path) -> Option<Guard> {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match create_exclusive(path) {
        Ok(()) => Some(Guard { path: path.to_path_buf() }),
        Err(()) => {
            if lock_is_stale(path) {
                let _ = fs::remove_file(path);
                create_exclusive(path).ok().map(|()| Guard { path: path.to_path_buf() })
            } else {
                None
            }
        }
    }
}

/// Atomically create the lockfile with our PID; `Err(())` if it already exists.
fn create_exclusive(path: &Path) -> Result<(), ()> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut f) => {
            let _ = write!(f, "{}", std::process::id());
            Ok(())
        }
        Err(_) => Err(()),
    }
}

/// Read the lockfile's PID + mtime age and classify staleness (thin IO around
/// `is_stale`). A missing/garbage lockfile is treated as stale (reapable).
fn lock_is_stale(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else { return true };
    let Ok(pid) = text.trim().parse::<u32>() else { return true };
    let age = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .unwrap_or(Duration::ZERO);
    is_stale(crate::server::lifecycle::pid_alive(pid), age, STALE_AGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_stale_true_when_owner_gone_or_too_old() {
        let cap = Duration::from_secs(600);
        assert!(is_stale(false, Duration::ZERO, cap)); // dead owner
        assert!(is_stale(true, Duration::from_secs(700), cap)); // over the cap
        assert!(!is_stale(true, Duration::from_secs(1), cap)); // live + young
    }

    #[test]
    fn default_path_is_under_cache_brain_sync() {
        assert!(default_path().ends_with(".cache/brain/sync/sync.lock"));
    }

    #[test]
    fn second_acquire_is_blocked_until_the_first_drops() {
        // The current process's PID is alive, so a held lock is not stale.
        let dir = std::env::temp_dir().join(format!("brain-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync.lock");

        let g1 = try_acquire(&path).expect("first acquire takes the lock");
        assert!(try_acquire(&path).is_none(), "second acquire is blocked by the live lock");
        drop(g1);
        let g3 = try_acquire(&path).expect("acquire succeeds after the first drops");
        drop(g3);

        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Add `pub mod lock;` to `src/sync/mod.rs`. Run: `cargo test --release sync::lock 2>&1 | tail -20`
Expected: compiles once wired; the 3 tests pass. (`crate::server::lifecycle::pid_alive` is already `pub` — confirm; if it is `pub(crate)` widen to `pub(crate)` reach or call `crate::state::system_pid_alive(pid as i32)` instead.)

- [ ] **Step 3: Write minimal implementation** — done in Step 1.
- [ ] **Step 4: Run test to verify it passes** — `cargo test --release sync::lock 2>&1 | tail -20` → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/sync/lock.rs src/sync/mod.rs
git commit -m "feat(sync): machine-wide advisory sync lock (atomic, reap-stale)"
```

### Task 2: wrap the manual `run_sync` in the lock

**Files:** Edit `src/main.rs`.

- [ ] **Step 1: RED via a behavior check.** There is no pure seam here (it is the CLI shell), so this is a wiring change verified by build + smoke, consistent with C2's dispatch tasks. Read the current `run_sync` in `src/main.rs` (around the `sync_command` handler).

- [ ] **Step 2: Edit.** In `run_sync`, **after** the `is_configured` guard (the `sync is not configured` early return) and **before** `sync_once`, acquire the lock; skip with a themed message if it is held:

```rust
    let _guard = match crate::sync::lock::try_acquire(&crate::sync::lock::default_path()) {
        Some(g) => g,
        None => {
            let theme = crate::theme::Theme::active();
            eprintln!("{}", theme.warning("another sync is already running; try again in a moment."));
            return Ok(());
        }
    };
```

`_guard` drops at the end of `run_sync`, releasing the lock. (Manual sync deliberately **skips** rather than waits — spec's skip-not-queue. A short blocking-wait for the human path is a possible later refinement, noted in §Open questions.)

- [ ] **Step 3: Build + smoke** (unconfigured path never spawns rclone or touches the lock's sync body):

```
cargo build --release 2>&1 | tail -3
./target/release/brain sync 2>&1 || true    # still prints "not configured" on this dev machine if unconfigured
```

- [ ] **Step 4: Full suite + clippy** — `cargo test --release 2>&1 | tail -6 && cargo clippy --release --all-targets 2>&1 | grep -cE "warning:"` → green, 0.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(sync): guard manual brain sync with the sync lock (closes the concurrent-run race)"
```

---

## C4.2 — The pure watcher core (`watch.rs`: `Debouncer` + predicate)

### Task 3: `Debouncer` + `is_watch_relevant` (no `notify` yet)

**Files:** Create `src/sync/watch.rs`; edit `src/sync/mod.rs`.

- [ ] **Step 1: Write the failing test**

Create `src/sync/watch.rs`:

```rust
//! The filesystem watcher: a **pure** debounce state machine + a path-relevance
//! predicate, plus (Task 5) the thin `notify` shell that feeds them. The watcher
//! runs in-process for the shell's lifetime; when it fires, one locked sync runs
//! synchronously in the watcher thread, so the sync's own writes buffer in the
//! event channel and coalesce into at most one no-op follow-up (no loop).

use std::path::{Component, Path};
use std::time::{Duration, Instant};

/// Coalesces a stream of filesystem events into "fire once things go quiet"
/// decisions. Pure: `now` is injected, so it tests without sleeps or a clock.
pub struct Debouncer {
    window: Duration,
    deadline: Option<Instant>,
}

impl Debouncer {
    #[must_use]
    pub fn new(window: Duration) -> Self {
        Self { window, deadline: None }
    }

    /// A relevant event arrived: (re)arm the quiescence timer.
    pub fn on_event(&mut self, now: Instant) {
        self.deadline = Some(now + self.window);
    }

    /// How long until a fire is due, or `None` when disarmed. `Some(0)` means
    /// "fire now" — the watcher loop uses this as its `recv_timeout`.
    #[must_use]
    pub fn time_until_fire(&self, now: Instant) -> Option<Duration> {
        self.deadline.map(|d| d.saturating_duration_since(now))
    }

    /// Fire iff the quiescence window has elapsed; disarms on fire.
    pub fn poll(&mut self, now: Instant) -> bool {
        match self.deadline {
            Some(d) if now >= d => {
                self.deadline = None;
                true
            }
            _ => false,
        }
    }

    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.deadline.is_some()
    }
}

/// Whether a changed path should trigger a sync. Mirrors the bisync exclude set
/// (spec §6): VCS/cache/OS cruft and existing conflict copies never trigger.
#[must_use]
pub fn is_watch_relevant(path: &Path) -> bool {
    for comp in path.components() {
        if let Component::Normal(os) = comp {
            let s = os.to_string_lossy();
            if s == ".git" || s == ".cache" || s == ".DS_Store" {
                return false;
            }
            if s.contains("(conflict ") && s.contains(')') {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disarmed_debouncer_never_fires() {
        let mut d = Debouncer::new(Duration::from_secs(3));
        assert!(!d.is_armed());
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
        assert!(!d.poll(t0 + Duration::from_secs(4)), "disarmed after firing");
    }

    #[test]
    fn a_burst_coalesces_into_a_single_fire() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::from_secs(3));
        d.on_event(t0);
        d.on_event(t0 + Duration::from_secs(1)); // re-arms → deadline = t0+4s
        d.on_event(t0 + Duration::from_secs(2)); // re-arms → deadline = t0+5s
        assert!(!d.poll(t0 + Duration::from_secs(4)), "still within the extended window");
        assert!(d.poll(t0 + Duration::from_secs(5)), "one fire once the burst settles");
        assert!(!d.poll(t0 + Duration::from_secs(6)));
    }

    #[test]
    fn time_until_fire_counts_down() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::from_secs(3));
        d.on_event(t0);
        assert_eq!(d.time_until_fire(t0), Some(Duration::from_secs(3)));
        assert_eq!(d.time_until_fire(t0 + Duration::from_secs(3)), Some(Duration::ZERO));
    }

    #[test]
    fn excludes_vcs_cache_os_cruft_and_conflict_copies() {
        assert!(!is_watch_relevant(Path::new(".git/index")));
        assert!(!is_watch_relevant(Path::new("notes/.DS_Store")));
        assert!(!is_watch_relevant(Path::new(".cache/x")));
        assert!(!is_watch_relevant(Path::new("notes/idea (conflict mac 2026-07-25).md")));
    }

    #[test]
    fn ordinary_notes_and_csvs_are_relevant() {
        assert!(is_watch_relevant(Path::new("projects/x/note.md")));
        assert!(is_watch_relevant(Path::new("tasks/tasks.csv")));
    }
}
```

- [ ] **Step 2: Run** — add `pub mod watch;` to `src/sync/mod.rs`; `cargo test --release sync::watch 2>&1 | tail -20` → 6 tests PASS.
- [ ] **Step 3: Implementation** — done in Step 1.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/sync/watch.rs src/sync/mod.rs
git commit -m "feat(sync): pure debounce state machine + watch-relevance predicate"
```

---

## C4.3 — The `notify` watcher shell + integration test

### Task 4: add `notify`; `spawn_watcher` + `WatcherHandle`

**Files:** Edit `Cargo.toml`; edit `src/sync/watch.rs`; add `tests/watch_local.rs`.

- [ ] **Step 1: Add the dependency.** In `Cargo.toml` `[dependencies]`:

```toml
# OS-native filesystem events (FSEvents / inotify) for the C4 auto-sync watcher.
# The only correct cross-platform FS-event crate; the alternative is a polling
# loop we reject as wasteful. We use the raw RecommendedWatcher and do the
# debouncing in our own tested `watch::Debouncer` (no notify-debouncer-* dep).
notify = "6"
```

> **Implementation note (verify at build time):** confirm the latest `notify` 6.x and its API (`notify::recommended_watcher(cb)`, `Watcher::watch(&path, RecursiveMode::Recursive)`, events as `Result<notify::Event, notify::Error>` with `event.paths: Vec<PathBuf>`). If 7.x is current and the API is compatible, prefer it; adjust the shell + this note together. `cargo build` after adding to confirm it resolves.

- [ ] **Step 2: Write the failing test** — a real FS-event test that drives the watcher through a **test callback** (no sync, no rclone, no B2).

Create `tests/watch_local.rs`:

```rust
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

    // Give the watcher a moment to register, then make a change.
    std::thread::sleep(Duration::from_millis(300));
    std::fs::write(root.join("note.md"), b"hello").unwrap();

    // Poll up to ~5s for the debounced fire (FSEvents can lag on macOS).
    let deadline = Instant::now() + Duration::from_secs(5);
    while fires.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(fires.load(Ordering::SeqCst) >= 1, "watcher should fire after a change");

    drop(handle); // stops the watcher thread without blocking teardown
    std::fs::remove_dir_all(&root).ok();
}
```

> **Note (test hygiene):** this is the single C4 integration test. It exercises the `notify`→`Debouncer`→fire wiring with a counter callback and a throwaway temp root; it must **never** be pointed at the real brain root or B2. If FS-event latency makes it flaky in CI, gate the body behind an env flag (e.g. `if std::env::var("BRAIN_WATCH_IT").is_err() { return; }`) as the sync integration test gates on rclone presence.

- [ ] **Step 3: Implement the thin shell.** Append to `src/sync/watch.rs`:

```rust
use std::path::PathBuf;
use std::sync::mpsc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Stops the watcher thread when dropped. Dropping the inner `Watcher` closes the
/// event channel, so the loop observes `Disconnected` and exits. We do **not**
/// join the thread — shell teardown must never block on an in-flight sync (spec
/// §10); a detached final pass is harmless (the lock coalesces it).
pub struct WatcherHandle {
    _watcher: RecommendedWatcher,
}

/// Start watching `root` recursively; call `on_fire` once each time changes
/// settle for `window`. The one relevant IO/thread shell over the pure
/// `Debouncer` + `is_watch_relevant`; `on_fire` runs synchronously in the loop,
/// so events during it buffer in the channel and coalesce (spec §6).
pub fn spawn_watcher_with<F>(root: &Path, window: Duration, on_fire: F) -> anyhow::Result<WatcherHandle>
where
    F: Fn() + Send + 'static,
{
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    std::thread::spawn(move || {
        let mut deb = Debouncer::new(window);
        loop {
            let now = Instant::now();
            // Block indefinitely when disarmed; else only until the fire is due.
            let recv = match deb.time_until_fire(now) {
                None => rx.recv().map_err(|_| ()),
                Some(d) => match rx.recv_timeout(d) {
                    Ok(ev) => Ok(ev),
                    Err(mpsc::RecvTimeoutError::Timeout) => Err(()), // maybe fire
                    Err(mpsc::RecvTimeoutError::Disconnected) => break, // handle dropped
                },
            };
            match recv {
                Ok(Ok(event)) => {
                    if event.paths.iter().any(|p| is_watch_relevant(p)) {
                        deb.on_event(Instant::now());
                    }
                }
                Ok(Err(_)) => {} // a notify error event; ignore
                Err(()) => {
                    // Either the channel closed (recv error) or a timeout elapsed.
                    if deb.poll(Instant::now()) {
                        on_fire();
                    } else {
                        // recv() returned Disconnected (handle dropped) → stop.
                        break;
                    }
                }
            }
        }
    });

    Ok(WatcherHandle { _watcher: watcher })
}

/// Start the real auto-sync watcher: fires a locked bidirectional sync when
/// changes under `root` settle for the configured debounce window.
pub fn spawn_watcher(root: &Path, cfg: &SyncConfig) -> anyhow::Result<WatcherHandle> {
    spawn_watcher_with(root, cfg.debounce(), || {
        crate::sync::trigger::run_locked_sync(crate::sync::args::Direction::Both);
    })
}
```

Add the imports the file now needs (`use crate::sync::config::SyncConfig;`). `PathBuf` is imported for signatures if used; drop it if unused to keep clippy clean.

> **Implementation note (the `Err(())` branch):** it means "either a debounce timeout OR the channel disconnected." Disambiguate as written: if `poll` fires, it was a timeout; otherwise `recv()` disconnected (handle dropped) → break. This keeps one match arm without a second flag. Verify the logic against the `disconnected` path in the test (`drop(handle)` must end the thread).

- [ ] **Step 4: Run.** `cargo build --release 2>&1 | tail -3` (resolves `notify`), then `cargo test --release --test watch_local 2>&1 | tail -20` → the fire test PASSES; `cargo test --release 2>&1 | tail -6` full green; `cargo clippy --release --all-targets 2>&1 | grep -cE "warning:"` → 0.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/sync/watch.rs tests/watch_local.rs
git commit -m "feat(sync): notify filesystem watcher shell + gated FS-event integration test"
```

---

## C4.4 — Triggers (`trigger.rs`) + `debounce_ms` config

### Task 5: `SyncConfig::debounce_ms` + `debounce()`

**Files:** Edit `src/sync/config.rs`.

- [ ] **Step 1: Write the failing test.** In `src/sync/config.rs`, add the field + method and a test.

Add to the struct:
```rust
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
```
Add the default + method:
```rust
fn default_debounce_ms() -> u64 {
    3000
}

impl SyncConfig {
    /// The watcher's quiescence window.
    #[must_use]
    pub fn debounce(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.debounce_ms)
    }
}
```
Add a test (and extend the existing `absent_fields_default_and_disable_sync` to assert the default):
```rust
    #[test]
    fn debounce_defaults_to_3s_and_maps_to_duration() {
        let c = parse("{}");
        assert_eq!(c.debounce_ms, 3000);
        assert_eq!(c.debounce(), std::time::Duration::from_millis(3000));
        let c2 = parse(r#"{"debounce_ms": 500}"#);
        assert_eq!(c2.debounce(), std::time::Duration::from_millis(500));
    }
```

- [ ] **Step 2: Run** → `cargo test --release sync::config 2>&1 | tail -20` RED (field missing) then GREEN.
- [ ] **Step 3: Implementation** — done in Step 1.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**

```bash
git add src/sync/config.rs
git commit -m "feat(sync): sync.debounce_ms config (default 3000) + debounce() helper"
```

### Task 6: `trigger.rs` — the locked/background/detached sync entry points

**Files:** Create `src/sync/trigger.rs`; edit `src/sync/mod.rs`.

- [ ] **Step 1: RED via a compile-guarded seam.** `trigger.rs` is a thin shell over already-tested pieces (`lock`, `sync_once`), so — per the house testing strategy (thin shells are exercised via integration, like C2's `run.rs` spawn and `setup::run`) — its coverage comes from Task 4's watcher integration test (which drives `run_locked_sync` via `spawn_watcher` on the real machine) plus `sync_once`'s own tests. The RED here is that `src/sync/watch.rs::spawn_watcher` (Task 4) references `trigger::run_locked_sync`, which does not exist yet → the build fails until this task lands. Confirm that failing build first: `cargo build --release 2>&1 | grep -i "run_locked_sync"`.

- [ ] **Step 2: Implement.** Create `src/sync/trigger.rs`:

```rust
//! Shell-facing sync triggers. All reuse `command::sync_once` under the sync
//! lock and are best-effort: a held lock, an unconfigured brain, or a spawn
//! failure is swallowed — a trigger never crashes or blocks the shell.

use std::process::{Command, Stdio};

use crate::sync::args::Direction;
use crate::sync::config::SyncConfig;
use crate::sync::{command, lock};

/// Run one sync now, under the lock, synchronously. No-op (returns immediately)
/// when sync is unconfigured or another sync holds the lock. Used by the watcher
/// (in its own thread) and by `sync_in_background`.
pub fn run_locked_sync(dir: Direction) {
    let cfg = SyncConfig::load();
    if !cfg.is_configured() {
        return;
    }
    let Some(_guard) = lock::try_acquire(&lock::default_path()) else {
        return; // another sync is running; skip (coalesce)
    };
    let Ok(root) = crate::paths::brain_root() else {
        return;
    };
    let now = chrono::Utc::now();
    let ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    let _ = command::sync_once(&cfg, &root, dir, (&ts, &ts, &date));
    // _guard drops here, releasing the lock.
}

/// Kick one background sync on a detached thread and return at once — used by the
/// `on_start` hook so shell startup never blocks on the network.
pub fn sync_in_background() {
    std::thread::spawn(|| run_locked_sync(Direction::Both));
}

/// Spawn `brain sync` as a fully detached child (own process group, null stdio)
/// so it outlives the shell — used by the `on_exit` hook. The child acquires the
/// lock itself; if a sync is already running it skips (that run covers the exit).
pub fn spawn_detached_sync() {
    use std::os::unix::process::CommandExt as _;
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe)
            .arg("sync")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn();
    }
}
```

Add `pub mod trigger;` to `src/sync/mod.rs`.

> **Test-safety note:** do **not** add a unit test that calls `run_locked_sync` / `sync_in_background` directly — they read the real brain-env `sync` block and could trigger a real B2 sync on this dev machine. Their correctness rests on the tested `lock`, `Debouncer`, and `sync_once`, and on Task 4's watcher integration test (which runs against a throwaway root only because its callback is the test counter, not `run_locked_sync`). If a direct test is ever wanted, sandbox `HOME`/`XDG_CONFIG_HOME` to a temp dir with **no** `sync` block so `is_configured()` is false and the body no-ops.

- [ ] **Step 3: Run** → `cargo build --release 2>&1 | tail -3` now compiles (`watch::spawn_watcher` resolves `trigger::run_locked_sync`); `cargo test --release 2>&1 | tail -6` green; clippy 0.
- [ ] **Step 4: (covered by build + Task 4's integration test).**
- [ ] **Step 5: Commit**

```bash
git add src/sync/trigger.rs src/sync/mod.rs
git commit -m "feat(sync): start/background/detached-exit sync triggers over the lock"
```

---

## C4.5 — TUI wiring + status line

### Task 7: wire the three triggers into `run_tui`

**Files:** Edit `src/tui/event_loop/setup.rs`.

- [ ] **Step 1: Behavior + build verification (no pure seam).** This is the shell wiring; verified by build + a manual run, like C2's dispatch. Read `run_tui` in `src/tui/event_loop/setup.rs` (already reviewed: builds the `App`, computes `brain_root`, runs `event_loop`, then restores the terminal).

- [ ] **Step 2: Edit.**

(a) **Keep `brain_root` after the `App::new` move.** `run_tui` currently moves `brain_root` into `App::new`. Clone it for the watcher: change the `App::new(..., brain_root, ...)` argument to `brain_root.clone()`, and keep the local `brain_root` binding alive for the wiring below. (Or read it back from `app` if it exposes the root; cloning the `PathBuf` is cheapest and least invasive.)

(b) **on_start + watcher**, after `app.seed_triage_day(...)` and **before** `let result = event_loop(...)`:

```rust
    // Auto-sync triggers (C4). All best-effort; none blocks the event loop.
    let sync_cfg = crate::sync::config::SyncConfig::load();
    if sync_cfg.on_start {
        crate::sync::trigger::sync_in_background();
    }
    let _watcher = if sync_cfg.watch_effective() {
        crate::sync::watch::spawn_watcher(&brain_root, &sync_cfg).ok()
    } else {
        None
    };
```

(c) **on_exit**, right after `let result = event_loop(&mut terminal, &mut app);` (before or after the session-lock release — order is immaterial, the detached child acquires the sync lock itself):

```rust
    if sync_cfg.on_exit {
        crate::sync::trigger::spawn_detached_sync();
    }
    drop(_watcher); // stop the watcher thread promptly (also drops at scope end)
```

Both hooks are already gated: `on_start`/`on_exit` are plain bools (default true), and `watch_effective()` already folds in `is_configured()`, so an unconfigured brain gets no watcher thread and no syncs. No new keybinding, palette row, or menu row — C4 adds no interactive surface.

- [ ] **Step 3: Build + manual smoke** (this machine has sync configured against real B2 — do **not** let a real sync run in an automated check; a build + a scoped unconfigured run is the safe verification):

```
cargo build --release 2>&1 | tail -3
# Confirm an UNCONFIGURED brain starts/quits unchanged (no watcher, no sync):
env HOME="$(mktemp -d)" XDG_CONFIG_HOME="$(mktemp -d)" ./target/release/brain tasks today --no-tui 2>&1 | tail -5 || true
```

Full interactive verification (open `brain`, edit a note, confirm a debounced sync, quit) is the user's to run on their real setup — flag it in the handoff; the automated suite must not touch real B2.

- [ ] **Step 4: Full suite + clippy** → green, 0 warnings.

- [ ] **Step 5: Commit**

```bash
git add src/tui/event_loop/setup.rs
git commit -m "feat(sync): wire on_start / watcher / on_exit auto-sync into the shell lifecycle"
```

### Task 8: extend `format_triggers` with the debounce window

**Files:** Edit `src/sync/command.rs`.

- [ ] **Step 1: Write the failing test.** `format_triggers` already renders on-start/on-exit/watch (reviewed). Add the debounce window and a test. In the `command.rs` tests module (or add one) assert the window shows when watch is on:

```rust
    #[test]
    fn format_triggers_shows_debounce_window_when_watch_on() {
        let cfg: SyncConfig =
            serde_json::from_str(r#"{"enabled":true,"b2_bucket":"b"}"#).unwrap();
        let line = format_triggers(&cfg, Theme::dark(false));
        assert!(line.contains("watch on"));
        assert!(line.contains("3000ms") || line.contains("3s"));
    }
```

- [ ] **Step 2: Run** → RED (no window in the string yet).

- [ ] **Step 3: Implement.** In `format_triggers`, append the window to the `watch` segment, e.g.:

```rust
        "{} on-start {} · on-exit {} · watch {} {}",
        theme.muted("triggers:"),
        yn(cfg.on_start),
        yn(cfg.on_exit),
        yn(cfg.watch_effective()),
        theme.muted(&format!("({}ms debounce)", cfg.debounce_ms)),
```

(Keep it tasteful — one muted parenthetical after the watch state. Match the exact assertion string you chose in Step 1: `3000ms`.)

- [ ] **Step 4: Run** → `cargo test --release sync::command 2>&1 | tail -20` PASS; clippy 0.

- [ ] **Step 5: Commit**

```bash
git add src/sync/command.rs
git commit -m "feat(sync): show the debounce window in brain sync status triggers line"
```

---

## C4.6 — Docs

### Task 9: documentation

**Files:** `docs/features.md`, `docs/architecture.md`, `docs/integrations.md`, `docs/data-model.md`, `docs/config.md`, `docs/decisions.md`, `AGENTS.md`.

- [ ] **Step 1: Full green + clippy** — `cargo test --release 2>&1 | tail -8 && cargo clippy --release --all-targets 2>&1 | grep -cE "warning:"` → all pass; 0.

- [ ] **Step 2: `docs/features.md`** — auto-sync on start (background) and exit (detached); the live watcher (default-on when configured, off with `sync.watch=false`); the `brain sync status` triggers line now shows the debounce window.

- [ ] **Step 3: `docs/architecture.md`** — the `notify` dependency **with its justification** (only correct cross-platform OS-native FS-event crate; polling rejected; raw watcher + our own `Debouncer`, no `notify-debouncer-*`); the C4 modules `src/sync/{lock,watch,trigger}.rs` and their pure/impure split; the `run_tui` lifecycle seam.

- [ ] **Step 4: `docs/integrations.md`** — the sync lock at `~/.cache/brain/sync/sync.lock` (PID file, reap-on-stale); the detached `on_exit` `brain sync` child (`process_group(0)`, null stdio); the watcher's exclude set (mirrors the bisync filter).

- [ ] **Step 5: `docs/data-model.md`** — the `sync.debounce_ms` env field (default 3000); the lockfile record (a bare PID).

- [ ] **Step 6: `docs/config.md`** — `sync.on_start` / `on_exit` / `watch` / `debounce_ms`: what each does, defaults (all on / 3000ms when configured), how to disable; note they are **brain env** `sync` fields (machine-local).

- [ ] **Step 7: `docs/decisions.md`** — in-process shell thread over a daemon (spec §3); detached fire-and-forget exit; event-driven `notify` over polling; skip-not-queue coalescing; the machine-wide sync lock (and that it also closes the pre-existing manual concurrent-sync race). *(Also fix the pre-existing duplicate `## C3` heading here if not already done — the progress/resume one was mislabeled.)*

- [ ] **Step 8: `AGENTS.md` docs-contract table** — add a C4 row: "the auto-sync triggers (start/exit hooks, the `notify` watcher + debounce, the sync lock)" → `docs/features.md` + `docs/architecture.md` + `docs/integrations.md` + `docs/decisions.md` (modules `src/sync/{lock,trigger,watch}.rs`; the seam in `src/tui/event_loop/setup.rs`).

- [ ] **Step 9: Commit**

```bash
git add docs/ AGENTS.md
git commit -m "docs: brain sync auto-triggers (C4) — start/exit hooks, notify watcher, sync lock"
```

---

## Self-Review

**Spec coverage (C4 slice):**
- C4 spec §2 forks (in-process thread / detached exit / event-driven 3s debounce / advisory lock) → Tasks 1–7.
- §4 module layout (`lock`/`trigger`/`watch` + config + TUI seam) → Tasks 1–8 (one responsibility per file).
- §5 the lock (atomic, non-blocking, reap-stale, skip-not-queue; wraps manual sync) → Tasks 1–2.
- §6 the watcher (`notify` + pure `Debouncer` + exclude predicate; synchronous-in-thread sync so self-writes coalesce to one no-op) → Tasks 3–4.
- §7 start/exit hooks → Tasks 6–7.
- §8 `status` triggers + debounce window → Task 8.
- §9 testing (pure `Debouncer`/predicate/`is_stale`/config; one gated FS-event integration; thin shells untested) → Tasks 1,3,4,5,8.
- §12 docs → Task 9.
- Deferred (idle-pull, standalone daemon) — absent, per §11.

**Placeholder scan:** No TBD/TODO in shipped code. Two `> Implementation note` callouts (Tasks 4) flag verifying the live `notify` version/API and the `Err(())` disambiguation branch — real build-time verification steps (spec §15), not placeholders; the shell is complete. The RED for the thin shells (`trigger`, the `notify` loop) is a failing **build** (an unresolved symbol) plus the Task-4 integration test, consistent with C2's treatment of `run.rs`/`setup.rs` shells.

**Type consistency:** `lock::{is_stale, try_acquire, Guard, default_path}`, `watch::{Debouncer, is_watch_relevant, spawn_watcher_with, spawn_watcher, WatcherHandle}`, `trigger::{run_locked_sync, sync_in_background, spawn_detached_sync}`, `SyncConfig::{debounce_ms, debounce}`, and the extended `format_triggers` are used with consistent names/signatures across tasks and callers (`watch::spawn_watcher` → `trigger::run_locked_sync`; `run_tui` → `sync_in_background`/`spawn_watcher`/`spawn_detached_sync`). `Direction` is reused from `crate::sync::args` (already `Copy + PartialEq`).

**Ordering:** Execute in order. Task 4 (`watch` shell) references `trigger::run_locked_sync`, created in Task 6 — Task 4 leaves the build RED (unresolved symbol) and Task 6 turns it GREEN; that is the intended red→green across the pair (Task 5 config lands between them and is independent). Task 7 (TUI) depends on Tasks 4+6; Task 8 depends on Task 5 (`debounce_ms`). Every `pub mod` in `src/sync/mod.rs` is added in the task that creates the file.

**Test safety (critical — real B2 on this machine):** No unit test calls a real sync path. The pure cores (`is_stale`, `Debouncer`, `is_watch_relevant`, `debounce`) are clock-injected and IO-free. The single integration test (`tests/watch_local.rs`) uses a throwaway temp root and a **counter callback**, never `run_locked_sync`, so it never loads the `sync` block, never spawns rclone, never touches B2. The `trigger` functions are deliberately left un-unit-tested (spec §9; §Test-safety note in Task 6). The Task-7 smoke runs an **unconfigured** brain under a temp `HOME`/`XDG_CONFIG_HOME`. Full interactive verification against real B2 is the user's to run.

**Prerequisite:** C2 + C3 merged to `main` (they are). C4 assumes `sync_once`, `run_sync`, `SyncConfig`, `paths::brain_root()`, `server::lifecycle::pid_alive`, `Theme`, and `format_triggers` exist.

## Open questions (resolve in the plan / at kickoff)

- **`notify` major version** — confirm 6.x vs 7.x against the current crates.io release at Task 4; the shell shape is version-stable, only the constructor/enum spellings may move.
- **Detach spelling** — `process_group(0)` + null stdio (mirrors `server::lifecycle::spawn_daemon`) is the settled approach; confirm it compiles on this macOS toolchain at Task 6.
- **Manual-sync skip vs. brief wait** — C4 ships skip-with-message (non-blocking `try_acquire`). If the human path feels abrupt, add an `acquire_blocking(path, ~2s)` poll-wait for the manual `run_sync` only (auto paths keep instant-skip). Left as a refinement.
- **`on_start` direction** — plain `Both` (bidirectional bisync reconciles both ways). Confirmed over `--pull`.
- **Watcher thread on exit** — drop-without-join (detached) to strictly honor "never block the shell"; the thread ends on channel-disconnect or process exit. Revisit only if a lingering thread ever causes a problem.
