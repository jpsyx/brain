//! Conflict-copy naming.
//!
//! rclone leaves the losing side of a same-file conflict with the
//! `args::CONFLICT_MARKER` suffix; we rewrite it to the friendly
//! `stem (conflict <host> <date>).ext`, and enumerate such copies for the
//! resolve flow (C5).

use std::fs;
use std::path::{Path, PathBuf};

use crate::sync::args::CONFLICT_MARKER;

/// Build the friendly conflict name for an original path.
///
/// Inserts ` (conflict <host> <date>)` before the extension.
/// `note.md` → `note (conflict mac 2026-07-25).md`; an extensionless
/// `README` → `README (conflict mac 2026-07-25)`.
#[must_use]
pub fn conflict_name(original: &Path, host: &str, date: &str) -> PathBuf {
    let dir = original.parent();
    let stem = original.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = original.extension().map(|e| e.to_string_lossy().into_owned());
    let tag = format!("{stem} (conflict {host} {date})");
    let name = match ext {
        Some(e) => format!("{tag}.{e}"),
        None => tag,
    };
    match dir {
        Some(d) if !d.as_os_str().is_empty() => d.join(name),
        _ => PathBuf::from(name),
    }
}

/// Given a marker file rclone produced (`<original><MARKER>`), compute the
/// friendly path to rename it to. Returns `None` if the path doesn't carry the
/// marker suffix.
#[must_use]
pub fn friendly_from_marker(marker_path: &Path, host: &str, date: &str) -> Option<PathBuf> {
    let s = marker_path.to_string_lossy();
    let original = s.strip_suffix(CONFLICT_MARKER)?;
    Some(conflict_name(Path::new(original), host, date))
}

/// Rename every `<path><MARKER>` file under `root` to its friendly conflict
/// name. Returns the count renamed. Best-effort: a failed rename is skipped.
pub fn rename_markers(root: &Path, host: &str, date: &str) -> usize {
    let mut n = 0;
    let walker = walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok);
    for entry in walker {
        let p = entry.path();
        if p.to_string_lossy().ends_with(CONFLICT_MARKER) {
            if let Some(dest) = friendly_from_marker(p, host, date) {
                if fs::rename(p, &dest).is_ok() {
                    n += 1;
                }
            }
        }
    }
    n
}

/// Count leftover marker files under `root` (used by verification).
#[must_use]
pub fn leftover_markers(root: &Path) -> usize {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().to_string_lossy().ends_with(CONFLICT_MARKER))
        .count()
}

/// An open conflict copy found under the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFile {
    pub path: PathBuf,
}

/// List conflict copies (`*(conflict *)*`) under `root`, as paths relative to
/// `root`.
#[must_use]
pub fn list_conflicts(root: &Path) -> Vec<ConflictFile> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name().to_string_lossy();
            n.contains("(conflict ") && n.contains(')')
        })
        .map(|e| ConflictFile { path: e.path().strip_prefix(root).unwrap_or_else(|_| e.path()).to_path_buf() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_conflicts_finds_friendly_named_copies_relative_to_root() {
        let tmp = std::env::temp_dir().join(format!("brain-listconflicts-{}", std::process::id()));
        let sub = tmp.join("notes");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("idea (conflict mac 2026-07-25).md"), b"x").unwrap();
        std::fs::write(sub.join("normal.md"), b"y").unwrap();

        let found = list_conflicts(&tmp);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, std::path::PathBuf::from("notes/idea (conflict mac 2026-07-25).md"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn inserts_conflict_tag_before_extension() {
        assert_eq!(
            conflict_name(Path::new("notes/idea.md"), "mac", "2026-07-25"),
            PathBuf::from("notes/idea (conflict mac 2026-07-25).md")
        );
    }

    #[test]
    fn handles_extensionless_files() {
        assert_eq!(
            conflict_name(Path::new("README"), "mac", "2026-07-25"),
            PathBuf::from("README (conflict mac 2026-07-25)")
        );
    }

    #[test]
    fn rewrites_a_marker_path_to_the_friendly_name() {
        let marker = PathBuf::from(format!("notes/idea.md{CONFLICT_MARKER}"));
        assert_eq!(
            friendly_from_marker(&marker, "mac", "2026-07-25"),
            Some(PathBuf::from("notes/idea (conflict mac 2026-07-25).md"))
        );
    }

    #[test]
    fn non_marker_path_yields_none() {
        assert_eq!(friendly_from_marker(Path::new("notes/idea.md"), "mac", "2026-07-25"), None);
    }

    #[test]
    fn rename_markers_moves_marker_files_to_friendly_names() {
        let tmp = std::env::temp_dir().join(format!("brain-conflicts-{}", std::process::id()));
        let sub = tmp.join("notes");
        std::fs::create_dir_all(&sub).unwrap();
        let marker = sub.join(format!("idea.md{CONFLICT_MARKER}"));
        std::fs::write(&marker, b"loser").unwrap();

        assert_eq!(leftover_markers(&tmp), 1);
        let n = rename_markers(&tmp, "mac", "2026-07-25");
        assert_eq!(n, 1);
        assert_eq!(leftover_markers(&tmp), 0);
        assert!(sub.join("idea (conflict mac 2026-07-25).md").exists());

        std::fs::remove_dir_all(&tmp).ok();
    }
}
