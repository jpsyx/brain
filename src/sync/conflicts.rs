//! Conflict-copy naming.
//!
//! rclone leaves the losing side of a same-file conflict with the
//! `args::CONFLICT_MARKER` suffix; we rewrite it to the friendly
//! `stem (conflict <host> <date>).ext`, and enumerate such copies for the
//! resolve flow (C5).

use std::fs;
use std::path::{Path, PathBuf};

use crate::sync::args::CONFLICT_MARKER;

/// Join `name` onto `dir` when present, else treat `name` as a bare path.
/// A no-op empty-check used to guard the `Some` branch, but
/// `Path::new("").join(name) == PathBuf::from(name)`, so `Path::parent()`'s
/// `Some("")` (single-component relative paths) and `None` both collapse to
/// the same result here.
fn join_dir(dir: Option<&Path>, name: &str) -> PathBuf {
    dir.map_or_else(|| PathBuf::from(name), |d| d.join(name))
}

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
    join_dir(dir, &name)
}

/// Recovered parts of a friendly conflict-copy name.
///
/// Not yet consumed outside this module — grouping (C5.1 Task 2) and the
/// `--json` surface (C5.2 Task 3) build on it next.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ParsedConflict {
    /// Canonical original this copy competes with, e.g. `notes/idea.md`.
    pub original: PathBuf,
    pub host: String,
    pub date: String,
}

/// Whether `date` is exactly `\d{4}-\d{2}-\d{2}` (no calendar validation).
///
/// Only called from `parse_conflict_name`, which isn't yet wired to a
/// caller — see the `#[allow(dead_code)]` note on `ParsedConflict` above.
#[allow(dead_code)]
fn is_conflict_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    bytes.len() == 10
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

/// Inverse of `conflict_name`: from `stem (conflict <host> <date>).ext` recover
/// the original path + host + date. Returns `None` when `path`'s file name isn't
/// the exact friendly-conflict grammar.
#[allow(dead_code)]
#[must_use]
pub fn parse_conflict_name(path: &Path) -> Option<ParsedConflict> {
    const OPEN: &str = " (conflict ";

    let name = path.file_name()?.to_str()?;
    let open_idx = name.rfind(OPEN)?;
    let stem = &name[..open_idx];
    let after_open = &name[open_idx + OPEN.len()..];

    let close_idx = after_open.find(')')?;
    let paren_content = &after_open[..close_idx];
    let ext = &after_open[close_idx + 1..];
    if !ext.is_empty() && !ext.starts_with('.') {
        return None;
    }

    let (host, date) = paren_content.rsplit_once(' ')?;
    if host.is_empty() || !is_conflict_date(date) {
        return None;
    }

    let name = format!("{stem}{ext}");
    let original = join_dir(path.parent(), &name);

    Some(ParsedConflict { original, host: host.to_string(), date: date.to_string() })
}

/// Strip rclone's marker segment `.<MARKER><digits>` off a file name, returning
/// the recovered original file name. rclone names the conflict loser
/// `<original>.<CONFLICT_MARKER><N>` (a literal dot, the suffix, and a trailing
/// integer `N` ≥ 1), e.g. `one.md` → `one.md.__brainconflict__1`. Returns
/// `None` if `name` is not a real marker.
fn strip_marker(name: &str) -> Option<String> {
    let without_digits = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if without_digits.len() == name.len() {
        return None; // no trailing integer → not a marker
    }
    without_digits.strip_suffix(&format!(".{CONFLICT_MARKER}")).map(ToOwned::to_owned)
}

/// Whether `path`'s file name is a real rclone conflict marker
/// (`<original>.<MARKER><digits>`).
#[must_use]
pub fn is_marker(path: &Path) -> bool {
    path.file_name().is_some_and(|n| strip_marker(&n.to_string_lossy()).is_some())
}

/// Given a marker file rclone produced (`<original>.<MARKER><N>`), compute the
/// friendly path to rename it to. Returns `None` if the path doesn't carry a
/// real marker suffix.
#[must_use]
pub fn friendly_from_marker(marker_path: &Path, host: &str, date: &str) -> Option<PathBuf> {
    let original_name = strip_marker(&marker_path.file_name()?.to_string_lossy())?;
    let original = join_dir(marker_path.parent(), &original_name);
    Some(conflict_name(&original, host, date))
}

/// Rename every `<path>.<MARKER><N>` file under `root` to its friendly conflict
/// name. Returns the count renamed. Best-effort: a failed rename is skipped.
pub fn rename_markers(root: &Path, host: &str, date: &str) -> usize {
    let mut n = 0;
    let walker = walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok);
    for entry in walker {
        let p = entry.path();
        if is_marker(p) {
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
        .filter(|e| is_marker(e.path()))
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
    fn round_trips_conflict_name_for_a_matrix() {
        for (orig, host, date) in [
            ("notes/idea.md", "mac", "2026-07-25"),
            ("README", "server-01", "2026-01-02"), // extensionless
            ("a/b c/my great note.md", "mac", "2026-12-31"), // spaces in stem + dir
            ("deep/nested/path/file.tar.gz", "mac", "2026-07-25"), // multi-dot ext
        ] {
            let built = conflict_name(Path::new(orig), host, date);
            let parsed = parse_conflict_name(&built).expect("should parse");
            assert_eq!(parsed.original, PathBuf::from(orig));
            assert_eq!(parsed.host, host);
            assert_eq!(parsed.date, date);
        }
    }

    #[test]
    fn rejects_non_conflict_names() {
        assert!(parse_conflict_name(Path::new("notes/idea.md")).is_none());
        // A real title that happens to mention a conflict but isn't the grammar.
        assert!(parse_conflict_name(Path::new("notes/the (conflict) resolution.md")).is_none());
        // rclone's raw marker is not a friendly copy.
        assert!(parse_conflict_name(Path::new(&format!("idea.md.{CONFLICT_MARKER}1"))).is_none());
    }

    #[test]
    fn rejects_malformed_date_inside_the_parens() {
        // Not zero-padded → doesn't match \d{4}-\d{2}-\d{2}.
        assert!(parse_conflict_name(Path::new("idea (conflict mac 2026-7-5).md")).is_none());
        // Letters where digits belong.
        assert!(parse_conflict_name(Path::new("idea (conflict mac 2026-AB-25).md")).is_none());
    }

    #[test]
    fn rejects_empty_host() {
        assert!(parse_conflict_name(Path::new("idea (conflict  2026-07-25).md")).is_none());
    }

    #[test]
    fn rejects_missing_closing_paren() {
        assert!(parse_conflict_name(Path::new("idea (conflict mac 2026-07-25.md")).is_none());
    }

    #[test]
    fn rejects_trailing_content_after_the_close_paren_that_isnt_an_extension() {
        // Non-empty, non-`.`-prefixed content after `)` fails the extension gate.
        assert!(parse_conflict_name(Path::new("idea (conflict mac 2026-07-25)x.md")).is_none());
    }

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
    fn rewrites_a_real_marker_path_to_the_friendly_name() {
        // Real rclone format: `<original>.<MARKER><N>` (literal dot + suffix +
        // trailing integer).
        let marker = PathBuf::from(format!("notes/idea.md.{CONFLICT_MARKER}1"));
        assert_eq!(
            friendly_from_marker(&marker, "mac", "2026-07-25"),
            Some(PathBuf::from("notes/idea (conflict mac 2026-07-25).md"))
        );
    }

    #[test]
    fn rewrites_a_multi_digit_marker() {
        let marker = PathBuf::from(format!("notes/idea.md.{CONFLICT_MARKER}12"));
        assert_eq!(
            friendly_from_marker(&marker, "mac", "2026-07-25"),
            Some(PathBuf::from("notes/idea (conflict mac 2026-07-25).md"))
        );
    }

    #[test]
    fn rewrites_an_extensionless_marker() {
        let marker = PathBuf::from(format!("README.{CONFLICT_MARKER}1"));
        assert_eq!(
            friendly_from_marker(&marker, "mac", "2026-07-25"),
            Some(PathBuf::from("README (conflict mac 2026-07-25)"))
        );
    }

    #[test]
    fn non_marker_path_yields_none() {
        assert_eq!(friendly_from_marker(Path::new("notes/idea.md"), "mac", "2026-07-25"), None);
        // marker text without a trailing digit is not a real rclone marker.
        assert_eq!(
            friendly_from_marker(Path::new(&format!("notes/idea.md.{CONFLICT_MARKER}")), "mac", "2026-07-25"),
            None
        );
    }

    #[test]
    fn is_marker_matches_only_the_real_shape() {
        assert!(is_marker(Path::new(&format!("idea.md.{CONFLICT_MARKER}1"))));
        assert!(is_marker(Path::new(&format!("README.{CONFLICT_MARKER}3"))));
        assert!(!is_marker(Path::new("idea.md")));
        assert!(!is_marker(Path::new(&format!("idea.md.{CONFLICT_MARKER}"))));
        assert!(!is_marker(Path::new(&format!("idea.md{CONFLICT_MARKER}1"))));
    }

    #[test]
    fn rename_markers_moves_real_marker_files_to_friendly_names() {
        let tmp = std::env::temp_dir().join(format!("brain-conflicts-{}", std::process::id()));
        let sub = tmp.join("notes");
        std::fs::create_dir_all(&sub).unwrap();
        let marker = sub.join(format!("idea.md.{CONFLICT_MARKER}1"));
        std::fs::write(&marker, b"loser").unwrap();
        let readme = sub.join(format!("README.{CONFLICT_MARKER}1"));
        std::fs::write(&readme, b"loser").unwrap();

        assert_eq!(leftover_markers(&tmp), 2);
        let n = rename_markers(&tmp, "mac", "2026-07-25");
        assert_eq!(n, 2);
        assert_eq!(leftover_markers(&tmp), 0);
        assert!(sub.join("idea (conflict mac 2026-07-25).md").exists());
        assert!(sub.join("README (conflict mac 2026-07-25)").exists());

        std::fs::remove_dir_all(&tmp).ok();
    }
}
