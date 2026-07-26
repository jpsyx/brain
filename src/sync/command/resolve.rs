//! `brain sync resolve <original> [...]`: safe local delete of conflict
//! copies once you've merged into the canonical original. Deletion only —
//! never runs a sync.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::sync::conflicts::{self, ConflictFile};
use crate::theme::Theme;

/// What resolving one original should do (pure classification, no fs).
#[derive(Debug, PartialEq, Eq)]
pub enum ResolveDecision {
    /// Delete these copies (canonical exists, copies found).
    Delete(Vec<PathBuf>),
    /// Refuse: the canonical original is missing — merge into it first.
    CanonicalMissing,
    /// Nothing to do: no copies for this original.
    NoCopies,
}

/// Decide what to do for one original.
///
/// `canonical_exists` is injected so this stays pure (no fs reads here). A
/// missing canonical always refuses, even when there are no copies either —
/// a bogus/mistyped original shouldn't silently no-op.
#[must_use]
pub fn resolve_decision(original: &Path, canonical_exists: bool, files: &[ConflictFile]) -> ResolveDecision {
    if !canonical_exists {
        return ResolveDecision::CanonicalMissing;
    }
    let copies = conflicts::copies_for_original(original, files);
    if copies.is_empty() { ResolveDecision::NoCopies } else { ResolveDecision::Delete(copies) }
}

/// `brain sync resolve <original> [...]`: delete the resolved conflict copies
/// for one or more canonical originals.
///
/// Only deletes; never runs a sync. Empty `originals` drops into an
/// interactive picker over the currently open conflict groups.
pub fn resolve(root: &Path, originals: &[String]) -> Result<()> {
    if originals.is_empty() {
        return resolve_interactive(root);
    }
    let theme = Theme::active();
    let files = conflicts::list_conflicts(root);
    resolve_many(root, originals, &files, theme);
    Ok(())
}

/// Apply the resolve decision to every original in `originals`, in order.
/// Shared by the explicit-args path and the interactive picker.
fn resolve_many(root: &Path, originals: &[String], files: &[ConflictFile], theme: Theme) {
    for original in originals {
        resolve_one(root, original, files, theme);
    }
}

/// Apply the resolve decision for one original: delete its copies (best
/// effort) and print a themed summary line. Never returns an error — an
/// individual delete failure is noted and resolution continues.
fn resolve_one(root: &Path, original: &str, files: &[ConflictFile], theme: Theme) {
    let rel = Path::new(original);
    let exists = root.join(rel).exists();
    match resolve_decision(rel, exists, files) {
        ResolveDecision::Delete(copies) => {
            let mut removed = 0usize;
            for copy in &copies {
                match fs::remove_file(root.join(copy)) {
                    Ok(()) => removed += 1,
                    Err(e) => {
                        eprintln!(
                            "{}",
                            theme.warning(&format!("could not remove {}: {e}", copy.display()))
                        );
                    }
                }
            }
            let word = if removed == 1 { "copy" } else { "copies" };
            println!(
                "{} {} {}",
                theme.success("resolved"),
                theme.value(original),
                theme.muted(&format!("(removed {removed} {word})")),
            );
        }
        ResolveDecision::CanonicalMissing => {
            println!(
                "{}",
                theme.warning(&format!(
                    "the canonical file {original} doesn't exist — merge into it before resolving"
                ))
            );
        }
        ResolveDecision::NoCopies => {
            println!("{}", theme.muted(&format!("no conflict copies for {original}")));
        }
    }
}

/// Interactive picker for bare `brain sync resolve` (no originals given).
/// Thin shell over `resolve_many`; never unit-tested (drives `/dev/tty`).
fn resolve_interactive(root: &Path) -> Result<()> {
    let theme = Theme::active();
    let files = conflicts::list_conflicts(root);
    let groups = conflicts::group_conflicts(&files);
    if groups.is_empty() {
        println!("{}", theme.muted("no open conflict copies."));
        return Ok(());
    }

    println!("{}", theme.heading("Open conflicts"));
    for (i, g) in groups.iter().enumerate() {
        let word = if g.copies.len() == 1 { "copy" } else { "copies" };
        println!(
            "  {} {} {}",
            theme.accent(&format!("{}.", i + 1)),
            theme.value(&g.original.display().to_string()),
            theme.muted(&format!("({} {word})", g.copies.len())),
        );
    }

    let answer =
        crate::sync::setup::prompt(&theme.prompt("Pick a number to resolve (or \"all\", empty to cancel)"), "")?;
    let answer = answer.trim();
    if answer.is_empty() {
        println!("{}", theme.muted("cancelled."));
        return Ok(());
    }

    let chosen: Vec<String> = if answer.eq_ignore_ascii_case("all") {
        groups.iter().map(|g| g.original.display().to_string()).collect()
    } else {
        match answer.parse::<usize>() {
            Ok(n) if n >= 1 && n <= groups.len() => vec![groups[n - 1].original.display().to_string()],
            _ => {
                println!("{}", theme.warning("not a valid choice."));
                return Ok(());
            }
        }
    };

    resolve_many(root, &chosen, &files, theme);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idea_conflict_files() -> Vec<ConflictFile> {
        vec![
            ConflictFile { path: PathBuf::from("idea (conflict mac 2026-07-25).md") },
            ConflictFile { path: PathBuf::from("idea (conflict server 2026-07-24).md") },
            ConflictFile { path: PathBuf::from("other (conflict mac 2026-07-25).md") },
        ]
    }

    #[test]
    fn resolve_decision_refuses_when_canonical_is_missing() {
        let files = idea_conflict_files();
        assert_eq!(
            resolve_decision(Path::new("idea.md"), false, &files),
            ResolveDecision::CanonicalMissing
        );
    }

    #[test]
    fn resolve_decision_deletes_copies_when_canonical_present() {
        let files = idea_conflict_files();
        assert_eq!(
            resolve_decision(Path::new("idea.md"), true, &files),
            ResolveDecision::Delete(vec![
                PathBuf::from("idea (conflict mac 2026-07-25).md"),
                PathBuf::from("idea (conflict server 2026-07-24).md"),
            ])
        );
    }

    #[test]
    fn resolve_decision_reports_no_copies_when_canonical_present_but_unmatched() {
        let files = idea_conflict_files();
        assert_eq!(resolve_decision(Path::new("nope.md"), true, &files), ResolveDecision::NoCopies);
    }

    #[test]
    fn resolve_decision_prefers_canonical_missing_over_no_copies() {
        // Guard precedence: a bogus original with no copies AND a missing
        // canonical is still refused, not silently treated as a no-op.
        let files = idea_conflict_files();
        assert_eq!(
            resolve_decision(Path::new("nope.md"), false, &files),
            ResolveDecision::CanonicalMissing
        );
    }

    #[test]
    fn resolve_deletes_the_copy_and_keeps_the_canonical() {
        // Hermetic fs check: real canonical + real conflict copy in a temp
        // dir, no rclone involved.
        let tmp = std::env::temp_dir().join(format!("brain-resolve-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("idea.md"), b"canonical").unwrap();
        fs::write(tmp.join("idea (conflict mac 2026-07-25).md"), b"loser").unwrap();

        resolve(&tmp, &["idea.md".to_owned()]).unwrap();

        assert!(tmp.join("idea.md").exists(), "canonical must survive resolve");
        assert!(
            !tmp.join("idea (conflict mac 2026-07-25).md").exists(),
            "the conflict copy must be deleted"
        );

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_deletes_copies_for_multiple_originals_in_one_call() {
        let tmp = std::env::temp_dir().join(format!("brain-resolve-many-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("idea.md"), b"merged idea").unwrap();
        fs::write(tmp.join("other.md"), b"merged other").unwrap();
        fs::write(tmp.join("idea (conflict mac 2026-07-25).md"), b"idea loser").unwrap();
        fs::write(tmp.join("other (conflict mac 2026-07-25).md"), b"other loser").unwrap();

        resolve(&tmp, &["idea.md".to_owned(), "other.md".to_owned()]).unwrap();

        assert!(tmp.join("idea.md").exists(), "first canonical must survive resolve");
        assert!(tmp.join("other.md").exists(), "second canonical must survive resolve");
        assert!(
            !tmp.join("idea (conflict mac 2026-07-25).md").exists(),
            "first conflict copy must be deleted"
        );
        assert!(
            !tmp.join("other (conflict mac 2026-07-25).md").exists(),
            "second conflict copy must be deleted"
        );

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_deletes_nested_subdir_conflict_copy() {
        let tmp = std::env::temp_dir().join(format!("brain-resolve-nested-{}", std::process::id()));
        let dir = tmp.join("projects");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("idea.md"), b"merged").unwrap();
        fs::write(dir.join("idea (conflict mac 2026-07-25).md"), b"loser").unwrap();

        resolve(&tmp, &["projects/idea.md".to_owned()]).unwrap();

        assert!(dir.join("idea.md").exists(), "nested canonical must survive resolve");
        assert!(
            !dir.join("idea (conflict mac 2026-07-25).md").exists(),
            "nested conflict copy must be deleted"
        );

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_leaves_everything_when_canonical_is_missing() {
        let tmp = std::env::temp_dir().join(format!("brain-resolve-missing-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("idea (conflict mac 2026-07-25).md"), b"loser").unwrap();

        resolve(&tmp, &["idea.md".to_owned()]).unwrap();

        assert!(
            tmp.join("idea (conflict mac 2026-07-25).md").exists(),
            "must refuse to delete when the canonical original is missing"
        );

        fs::remove_dir_all(&tmp).ok();
    }
}
