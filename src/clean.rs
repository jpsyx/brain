//! `brain clean` — remove tool byproducts from the workspace root.
//!
//! Tools scatter artifacts through a brain that have no notes value: macOS
//! Finder metadata, Python caches, editor scratch. They pollute searches, bloat
//! backups and syncs, and clutter every listing.
//!
//! The pattern list is deliberately **conservative and closed**. Everything on
//! it is an artifact a tool created and can recreate, recognizable by name
//! alone — never a file whose name merely looks generated. Deleting a note
//! someone wrote is unrecoverable; leaving a stray cache is a rounding error,
//! so the list only grows for things that are unambiguously regenerable.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

/// Directory names removed wholesale.
const CACHE_DIRECTORIES: [&str; 6] = [
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".ipynb_checkpoints",
    ".DS_Store.d",
];

/// File names removed.
const JUNK_FILES: [&str; 3] = [".DS_Store", "Thumbs.db", ".localized"];

/// Directories never descended into: their contents are not ours to judge.
const NEVER_ENTER: [&str; 4] = [".git", ".agents", ".claude", ".codex"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Removal {
    /// Path relative to the workspace root.
    pub path: String,
    pub is_directory: bool,
}

/// Pure: does this entry name a byproduct?
#[must_use]
pub fn is_byproduct(name: &str, is_directory: bool) -> bool {
    if is_directory {
        CACHE_DIRECTORIES.contains(&name)
    } else {
        JUNK_FILES.contains(&name)
    }
}

/// Pure: should the walk descend into this directory?
#[must_use]
pub fn should_enter(name: &str) -> bool {
    !NEVER_ENTER.contains(&name)
}

/// Every byproduct under `root`, in walk order.
#[must_use]
pub fn find(root: &Path) -> Vec<Removal> {
    let mut found = Vec::new();
    collect(root, root, &mut found);
    found
}

fn collect(root: &Path, directory: &Path, found: &mut Vec<Removal>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_directory = path.is_dir();
        if is_byproduct(&name, is_directory) {
            found.push(Removal {
                path: path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
                is_directory,
            });
            continue;
        }
        // A symlink is never followed: a link out of the brain is not ours.
        if is_directory && !path.is_symlink() && should_enter(&name) {
            collect(root, &path, found);
        }
    }
}

/// Remove every byproduct under `root`, or with `dry_run` only list them.
pub fn run(root: &Path, dry_run: bool) -> Result<Vec<Removal>> {
    let found = find(root);
    if dry_run {
        return Ok(found);
    }
    for removal in &found {
        let path = PathBuf::from(root).join(&removal.path);
        let outcome = if removal.is_directory {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(error) = outcome {
            crate::logging::log(format!(
                "clean: could not remove {}: {error}",
                path.display()
            ));
        }
    }
    Ok(found)
}

/// Pure: the themed report.
#[must_use]
pub fn render(removals: &[Removal], dry_run: bool, theme: crate::theme::Theme) -> String {
    use std::fmt::Write as _;

    if removals.is_empty() {
        return format!("{}\n", theme.success("The brain is already clean."));
    }
    let verb = if dry_run { "Would remove" } else { "Removed" };
    let mut out = format!(
        "{}\n",
        theme.success(&format!("{verb} {} tool byproduct(s):", removals.len()))
    );
    for removal in removals {
        let _ = writeln!(out, "  {} {}", theme.muted("-"), theme.value(&removal.path));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Removal, find, is_byproduct, render, run, should_enter};
    use crate::theme::Theme;

    fn brain() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn caches_and_finder_metadata_are_byproducts() {
        assert!(is_byproduct("__pycache__", true));
        assert!(is_byproduct(".pytest_cache", true));
        assert!(is_byproduct(".DS_Store", false));
    }

    #[test]
    fn a_note_that_merely_looks_generated_is_not_a_byproduct() {
        // Deleting something someone wrote is unrecoverable.
        assert!(!is_byproduct("cache.md", false));
        assert!(!is_byproduct("__pycache__.md", false));
        assert!(!is_byproduct("notes", true));
        assert!(!is_byproduct("pipeline.json", false));
    }

    #[test]
    fn a_directory_name_is_not_a_file_name() {
        assert!(!is_byproduct("__pycache__", false));
        assert!(!is_byproduct(".DS_Store", true));
    }

    #[test]
    fn the_walk_never_enters_git_or_the_agent_registries() {
        for name in [".git", ".agents", ".claude", ".codex"] {
            assert!(!should_enter(name), "{name}");
        }
        assert!(should_enter("projects"));
    }

    #[test]
    fn byproducts_are_found_at_every_depth() {
        let dir = brain();
        let root = dir.path();
        std::fs::create_dir_all(root.join("projects/site/__pycache__")).expect("dirs");
        std::fs::write(root.join("projects/site/__pycache__/x.pyc"), "").expect("pyc");
        std::fs::write(root.join(".DS_Store"), "").expect("ds");
        std::fs::write(root.join("projects/site/notes.md"), "keep me").expect("note");

        let found = find(root);

        assert_eq!(
            found,
            [
                Removal {
                    path: ".DS_Store".to_owned(),
                    is_directory: false
                },
                Removal {
                    path: "projects/site/__pycache__".to_owned(),
                    is_directory: true
                },
            ]
        );
    }

    #[test]
    fn a_dry_run_changes_nothing() {
        let dir = brain();
        let root = dir.path();
        std::fs::write(root.join(".DS_Store"), "").expect("ds");

        assert_eq!(run(root, true).expect("dry run").len(), 1);
        assert!(root.join(".DS_Store").exists());
    }

    #[test]
    fn cleaning_removes_the_byproducts_and_nothing_else() {
        let dir = brain();
        let root = dir.path();
        std::fs::create_dir_all(root.join("projects/site/__pycache__")).expect("dirs");
        std::fs::write(root.join("projects/site/__pycache__/x.pyc"), "").expect("pyc");
        std::fs::write(root.join("projects/site/notes.md"), "keep me").expect("note");
        std::fs::write(root.join(".DS_Store"), "").expect("ds");

        run(root, false).expect("clean");

        assert!(!root.join(".DS_Store").exists());
        assert!(!root.join("projects/site/__pycache__").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("projects/site/notes.md")).expect("note"),
            "keep me"
        );
    }

    #[test]
    fn cleaning_is_a_no_op_the_second_time() {
        let dir = brain();
        let root = dir.path();
        std::fs::write(root.join(".DS_Store"), "").expect("ds");

        run(root, false).expect("clean");

        assert!(run(root, false).expect("clean again").is_empty());
    }

    #[test]
    fn a_clean_brain_says_so_rather_than_printing_an_empty_list() {
        assert_eq!(
            render(&[], false, Theme::dark(false)),
            "The brain is already clean.\n"
        );
    }

    #[test]
    fn the_report_distinguishes_a_preview_from_a_deletion() {
        let removals = [Removal {
            path: ".DS_Store".to_owned(),
            is_directory: false,
        }];
        assert!(render(&removals, true, Theme::dark(false)).contains("Would remove 1"));
        assert!(render(&removals, false, Theme::dark(false)).contains("Removed 1"));
    }
}
