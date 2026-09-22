//! Where the tree is rooted, and how far up it may walk.
//!
//! The brain root is the ceiling: `ascend` refuses to return a parent at or
//! above it, which is what makes the `../` row disappear there and what keeps
//! the tree inside the workspace no matter how it is entered.

use std::path::{Path, PathBuf};

use crate::entry::Entry;

/// Where the tree opens for the search scope currently loaded.
///
/// `picker::App` stores no scope, only entries, each tagged with its bucket.
/// Entries spanning exactly one bucket mean a scoped search, so the tree opens
/// at that bucket's directory; anything else (several buckets, or none) opens
/// at the brain root. Deriving it keeps one source of truth, rather than a
/// stored scope that could drift from the entries themselves.
pub(crate) fn scope_root(entries: &[Entry], brain_root: &Path) -> PathBuf {
    let mut buckets = entries.iter().map(|entry| entry.bucket);
    let Some(first) = buckets.next() else {
        return brain_root.to_path_buf();
    };
    if buckets.any(|bucket| bucket != first) {
        return brain_root.to_path_buf();
    }
    brain_root.join(first.dir_name())
}

/// Whether `target` sits inside the tree `entries` would open.
///
/// The palette's target picker walks every bucket, so it can hand back a path
/// the current search scope does not contain. A caller that gets `false` here
/// has to widen the entry set, or the tree would open on the wrong root with
/// nothing selected.
pub(crate) fn covers(entries: &[Entry], brain_root: &Path, target: &Path) -> bool {
    target.starts_with(scope_root(entries, brain_root))
}

/// The directory one level above `root`, or `None` when there is nowhere
/// legal to go: `root` is the brain root, or sits outside it entirely.
///
/// This is the single place the "never above the brain root" rule lives.
pub(crate) fn ascend(root: &Path, brain_root: &Path) -> Option<PathBuf> {
    if !root.starts_with(brain_root) || root == brain_root {
        return None;
    }
    root.parent()
        .filter(|parent| parent.starts_with(brain_root))
        .map(Path::to_path_buf)
}

/// Whether the tree shows its synthetic `../` row. Defined in terms of
/// [`ascend`] so the row can never offer a move the model would refuse.
pub(crate) fn shows_parent_row(root: &Path, brain_root: &Path) -> bool {
    ascend(root, brain_root).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Bucket;

    fn entry(path: &str, bucket: Bucket) -> Entry {
        Entry {
            path: PathBuf::from(path),
            display: path.to_owned(),
            bucket,
            is_dir: false,
            is_hidden: false,
        }
    }

    #[test]
    fn one_bucket_in_scope_roots_at_that_bucket_directory() {
        let entries = vec![
            entry("/brain/projects/atlas/plan.md", Bucket::Projects),
            entry("/brain/projects/atlas", Bucket::Projects),
        ];
        assert_eq!(
            scope_root(&entries, Path::new("/brain")),
            PathBuf::from("/brain/projects")
        );
    }

    #[test]
    fn several_buckets_in_scope_root_at_the_brain_root() {
        let entries = vec![
            entry("/brain/projects/atlas", Bucket::Projects),
            entry("/brain/areas/health", Bucket::Areas),
        ];
        assert_eq!(
            scope_root(&entries, Path::new("/brain")),
            PathBuf::from("/brain")
        );
    }

    #[test]
    fn no_entries_root_at_the_brain_root() {
        assert_eq!(
            scope_root(&[], Path::new("/brain")),
            PathBuf::from("/brain")
        );
    }

    #[test]
    fn a_single_bucket_scope_covers_that_bucket_and_nothing_else() {
        let entries = vec![entry("/brain/capture/inbox.md", Bucket::Capture)];
        let brain = Path::new("/brain");

        assert!(covers(
            &entries,
            brain,
            Path::new("/brain/capture/inbox.md")
        ));
        assert!(covers(
            &entries,
            brain,
            Path::new("/brain/capture/deep/note.md")
        ));
        assert!(!covers(
            &entries,
            brain,
            Path::new("/brain/projects/plan.md")
        ));
    }

    #[test]
    fn a_scope_spanning_buckets_or_holding_nothing_covers_the_whole_brain_root() {
        let spanning = vec![
            entry("/brain/projects/atlas", Bucket::Projects),
            entry("/brain/areas/health", Bucket::Areas),
        ];
        let brain = Path::new("/brain");

        assert!(covers(
            &spanning,
            brain,
            Path::new("/brain/projects/plan.md")
        ));
        assert!(covers(
            &spanning,
            brain,
            Path::new("/brain/capture/inbox.md")
        ));
        assert!(covers(&[], brain, Path::new("/brain/capture/inbox.md")));
        assert!(!covers(&[], brain, Path::new("/etc/passwd")));
    }

    #[test]
    fn ascend_returns_the_parent_below_the_brain_root() {
        assert_eq!(
            ascend(Path::new("/brain/projects/atlas"), Path::new("/brain")),
            Some(PathBuf::from("/brain/projects"))
        );
        assert_eq!(
            ascend(Path::new("/brain/projects"), Path::new("/brain")),
            Some(PathBuf::from("/brain"))
        );
    }

    #[test]
    fn ascend_refuses_to_leave_the_brain_root() {
        // At the ceiling there is nowhere to go.
        assert_eq!(ascend(Path::new("/brain"), Path::new("/brain")), None);
        // And a path that was never inside it cannot climb into one.
        assert_eq!(ascend(Path::new("/etc/passwd"), Path::new("/brain")), None);
        assert_eq!(ascend(Path::new("/"), Path::new("/brain")), None);
    }

    #[test]
    fn a_sibling_whose_name_merely_starts_with_the_brain_root_is_still_outside() {
        // `Path::starts_with` matches whole components, not bytes, so
        // `/brainstorming` is not inside `/brain`. Pinning it because a
        // byte-wise prefix check here would silently let the tree escape the
        // workspace into a same-prefixed sibling.
        let brain = Path::new("/brain");

        assert_eq!(ascend(Path::new("/brainstorming/notes"), brain), None);
        assert!(!shows_parent_row(Path::new("/brainstorming"), brain));
    }

    #[test]
    fn the_parent_row_shows_exactly_when_there_is_somewhere_to_ascend_to() {
        for (root, expected) in [
            ("/brain/projects/atlas", true),
            ("/brain/projects", true),
            ("/brain", false),
            ("/etc", false),
        ] {
            let root = Path::new(root);
            let brain = Path::new("/brain");
            assert_eq!(shows_parent_row(root, brain), expected, "{root:?}");
            assert_eq!(shows_parent_row(root, brain), ascend(root, brain).is_some());
        }
    }
}
