//! Out-of-band max-merge for the monotonic id counters
//! (`tasks/.tasks_next_id`, `tasks/.habits_next_id`).
//!
//! These hold the next integer id to hand out for a new task / habit. Line-based
//! bisync would resolve a two-machine divergence by *newer mtime*, which is
//! wrong for a counter: if the machine with the **lower** value wrote more
//! recently, its value would win and it would then re-hand-out ids the other
//! machine already assigned, colliding in the id-keyed CSV merge. So the
//! counters are excluded from bisync (see [`crate::sync::args`]) and reconciled
//! here by the only rule that can't lose an id: **take the highest**.
//!
//! Max-merge is stateless (no 3-way baseline): `max(local, remote)` is
//! convergent, idempotent, and monotonic, so it never regresses a counter.

use std::path::{Path, PathBuf};

use crate::sync::args::Direction;
use crate::sync::config::SyncConfig;
use crate::sync::remote::build_remote;
use crate::sync::run::run_rclone_capture;

/// The two id-counter files reconciled out-of-band, as repo-relative paths.
pub const COUNTERS: [&str; 2] = ["tasks/.tasks_next_id", "tasks/.habits_next_id"];

/// Parse a counter file's contents to an integer (`None` if empty/garbage).
#[must_use]
pub fn parse_counter(text: &str) -> Option<u32> {
    text.trim().parse::<u32>().ok()
}

/// The reconciled counter: the highest of whichever sides are present. `None`
/// only when neither side has a usable value (leave the file absent, so id
/// allocation falls back to `max_existing_id + 1`).
#[must_use]
pub fn merge_counter(local: Option<u32>, remote: Option<u32>) -> Option<u32> {
    match (local, remote) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// The machine-local path for a counter, given the brain root and repo-relative
/// name.
#[must_use]
pub fn counter_path(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

/// Reconcile ONE counter via max-merge. `fetch`/`push` are injected so the real
/// path uses rclone `copyto` and tests use local values. Returns the reconciled
/// value (if any).
pub fn sync_one_counter(
    local: &Path,
    fetch: impl Fn() -> Option<String>,
    push: impl Fn(&str) -> bool,
) -> Option<u32> {
    sync_one_counter_with_mode(local, fetch, push, true)
}

fn sync_one_counter_push_only(
    local: &Path,
    fetch: impl Fn() -> Option<String>,
    push: impl Fn(&str) -> bool,
) -> Option<u32> {
    sync_one_counter_with_mode(local, fetch, push, false)
}

fn sync_one_counter_with_mode(
    local: &Path,
    fetch: impl Fn() -> Option<String>,
    push: impl Fn(&str) -> bool,
    update_local: bool,
) -> Option<u32> {
    let ours = std::fs::read_to_string(local).ok().and_then(|t| parse_counter(&t));
    let theirs = fetch().and_then(|t| parse_counter(&t));
    let merged = merge_counter(ours, theirs);
    if let Some(value) = merged {
        // Only write when the merged value differs from what each side already
        // holds, so an unchanged counter doesn't churn the file or the remote.
        let text = format!("{value}\n");
        if update_local && ours != Some(value) {
            if let Some(dir) = local.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(local, &text);
        }
        if theirs != Some(value) {
            let _ = push(&text);
        }
    }
    merged
}

/// Max-merge both id counters against the remote, wiring `fetch`/`push` to
/// rclone `copyto` through a temp file. Best-effort: a per-counter failure
/// yields `None` rather than aborting the caller's sync.
#[must_use]
pub fn sync_counters(
    cfg: &SyncConfig,
    root: &Path,
    direction: Direction,
) -> Vec<(String, Option<u32>)> {
    let remote = build_remote(cfg);
    let mut out = Vec::with_capacity(COUNTERS.len());
    for rel in COUNTERS {
        let local = counter_path(root, rel);
        let remote_arg = crate::sync::csv_sync::remote_csv_arg(&remote.arg, rel);
        let tag = rel.replace('/', "_");

        let fetch = || {
            let tmp = std::env::temp_dir()
                .join(format!("brain-counter-fetch-{}-{tag}", std::process::id()));
            let args =
                ["copyto".to_owned(), remote_arg.clone(), tmp.to_string_lossy().into_owned()];
            let (ok, _) = run_rclone_capture(&remote.env, &args);
            let text = ok.then(|| std::fs::read_to_string(&tmp).ok()).flatten();
            let _ = std::fs::remove_file(&tmp);
            text
        };
        let push = |text: &str| {
            let tmp = std::env::temp_dir()
                .join(format!("brain-counter-push-{}-{tag}", std::process::id()));
            if std::fs::write(&tmp, text).is_err() {
                return false;
            }
            let args =
                ["copyto".to_owned(), tmp.to_string_lossy().into_owned(), remote_arg.clone()];
            let (ok, _) = run_rclone_capture(&remote.env, &args);
            let _ = std::fs::remove_file(&tmp);
            ok
        };

        let value = if direction == Direction::Push {
            sync_one_counter_push_only(&local, fetch, push)
        } else {
            sync_one_counter(&local, fetch, push)
        };
        out.push((rel.to_owned(), value));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_takes_the_highest_present_value() {
        assert_eq!(merge_counter(Some(51), Some(31)), Some(51));
        assert_eq!(merge_counter(Some(31), Some(51)), Some(51));
        assert_eq!(merge_counter(Some(7), Some(7)), Some(7));
    }

    #[test]
    fn merge_uses_whichever_side_is_present_when_one_is_missing() {
        assert_eq!(merge_counter(Some(51), None), Some(51));
        assert_eq!(merge_counter(None, Some(31)), Some(31));
        assert_eq!(merge_counter(None, None), None);
    }

    #[test]
    fn parse_ignores_whitespace_and_garbage() {
        assert_eq!(parse_counter("52\n"), Some(52));
        assert_eq!(parse_counter("  7 "), Some(7));
        assert_eq!(parse_counter(""), None);
        assert_eq!(parse_counter("H31"), None);
    }

    #[test]
    fn sync_one_counter_writes_the_max_locally_and_pushes_it() {
        use std::cell::RefCell;
        let dir = std::env::temp_dir().join(format!("brain-counter-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let local = dir.join(".habits_next_id");
        std::fs::write(&local, "31\n").unwrap();

        // Remote is higher: local must be raised to it and the same value pushed.
        let pushed: RefCell<Option<String>> = RefCell::new(None);
        let merged = sync_one_counter(
            &local,
            || Some("51\n".to_owned()),
            |t| {
                *pushed.borrow_mut() = Some(t.to_owned());
                true
            },
        );
        assert_eq!(merged, Some(51));
        assert_eq!(std::fs::read_to_string(&local).unwrap(), "51\n");
        // The remote already held the max, so there's nothing to push back up
        // (churn avoidance) — only the lagging local side was raised.
        assert_eq!(pushed.borrow().as_deref(), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sync_one_counter_pushes_up_when_local_is_higher() {
        use std::cell::RefCell;
        let dir = std::env::temp_dir().join(format!("brain-counter-up-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let local = dir.join(".tasks_next_id");
        std::fs::write(&local, "99\n").unwrap();

        let pushed: RefCell<Option<String>> = RefCell::new(None);
        let merged = sync_one_counter(
            &local,
            || Some("40\n".to_owned()),
            |t| {
                *pushed.borrow_mut() = Some(t.to_owned());
                true
            },
        );
        assert_eq!(merged, Some(99));
        // Local already held the max, so it isn't rewritten needlessly, but the
        // higher value is pushed up to the lagging remote.
        assert_eq!(std::fs::read_to_string(&local).unwrap(), "99\n");
        assert_eq!(pushed.borrow().as_deref(), Some("99\n"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn push_only_counter_does_not_download_a_higher_remote_value() {
        let dir =
            std::env::temp_dir().join(format!("brain-counter-push-only-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let local = dir.join(".tasks_next_id");
        std::fs::write(&local, "40\n").unwrap();

        let merged = sync_one_counter_push_only(&local, || Some("99\n".to_owned()), |_| true);

        assert_eq!(merged, Some(99));
        assert_eq!(std::fs::read_to_string(&local).unwrap(), "40\n");
        std::fs::remove_dir_all(dir).ok();
    }
}
