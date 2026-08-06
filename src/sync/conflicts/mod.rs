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
    let stem = original
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = original
        .extension()
        .map(|e| e.to_string_lossy().into_owned());
    let tag = format!("{stem} (conflict {host} {date})");
    let name = match ext {
        Some(e) => format!("{tag}.{e}"),
        None => tag,
    };
    join_dir(dir, &name)
}

/// Recovered parts of a friendly conflict-copy name.
///
/// Consumed within this module by `group_conflicts`/`copies_for_original`,
/// and reachable from the bin via `print_conflicts`'s `--json` branch
/// (`group_conflicts` → `parse_conflict_name`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ParsedConflict {
    /// Canonical original this copy competes with, e.g. `notes/idea.md`.
    pub original: PathBuf,
    pub host: String,
    pub date: String,
}

/// Whether `date` is exactly `\d{4}-\d{2}-\d{2}` (no calendar validation).
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

    Some(ParsedConflict {
        original,
        host: host.to_string(),
        date: date.to_string(),
    })
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
    without_digits
        .strip_suffix(&format!(".{CONFLICT_MARKER}"))
        .map(ToOwned::to_owned)
}

/// Whether `path`'s file name is a real rclone conflict marker
/// (`<original>.<MARKER><digits>`).
#[must_use]
pub fn is_marker(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| strip_marker(&n.to_string_lossy()).is_some())
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
    let walker = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok);
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
        .map(|e| ConflictFile {
            path: e
                .path()
                .strip_prefix(root)
                .unwrap_or_else(|_| e.path())
                .to_path_buf(),
        })
        .collect()
}

/// A canonical original and its open conflict copies.
///
/// Consumed by `crate::sync::command::conflicts_json` for `brain sync
/// conflicts --json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConflictGroup {
    pub original: PathBuf,
    pub copies: Vec<ParsedCopy>,
}

/// One conflict copy within a [`ConflictGroup`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ParsedCopy {
    /// Relative to root, as it came from `ConflictFile`.
    pub path: PathBuf,
    pub host: String,
    pub date: String,
}

/// Fold flat `list_conflicts` output into groups keyed by recovered original.
///
/// Copies whose name doesn't parse as a friendly conflict are dropped. Output
/// is sorted by original, and by path within each group, so it's deterministic
/// regardless of input order (serialized to JSON by `--json`, where
/// byte-stable output matters).
#[must_use]
pub fn group_conflicts(files: &[ConflictFile]) -> Vec<ConflictGroup> {
    let mut groups: Vec<ConflictGroup> = Vec::new();
    for file in files {
        let Some(parsed) = parse_conflict_name(&file.path) else {
            continue;
        };
        let copy = ParsedCopy {
            path: file.path.clone(),
            host: parsed.host,
            date: parsed.date,
        };
        match groups.iter_mut().find(|g| g.original == parsed.original) {
            Some(group) => group.copies.push(copy),
            None => groups.push(ConflictGroup {
                original: parsed.original,
                copies: vec![copy],
            }),
        }
    }
    groups.sort_by(|a, b| a.original.cmp(&b.original));
    for group in &mut groups {
        group.copies.sort_by(|a, b| a.path.cmp(&b.path));
    }
    groups
}

/// The copies (from the live conflict set) belonging to `original`, matched via
/// the recovered `ParsedConflict.original`. Never returns `original` itself.
///
/// Consumed by `sync::command::resolve_decision` for `brain sync resolve
/// <original>` (C5.3 Task 4).
#[must_use]
pub fn copies_for_original(original: &Path, files: &[ConflictFile]) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|f| parse_conflict_name(&f.path).is_some_and(|p| p.original == original))
        .map(|f| f.path.clone())
        .collect()
}

#[cfg(test)]
mod tests;
