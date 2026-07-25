//! Conflict-copy naming.
//!
//! rclone leaves the losing side of a same-file conflict with the
//! `args::CONFLICT_MARKER` suffix; we rewrite it to the friendly
//! `stem (conflict <host> <date>).ext`, and enumerate such copies for the
//! resolve flow (C5).

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
