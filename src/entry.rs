//! Walk one or more root directories and collect entries (files + dirs).
//!
//! Hidden files (`.git`, `.DS_Store`, anything starting with `.`) are skipped,
//! matching the `fd .` default that the previous zsh helper relied on. Each
//! entry is tagged with its `Bucket` so the picker can group results into
//! Capture / Projects / Areas / Resources sections.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::{DirEntry, WalkDir};

/// Top-level bucket inside `~/brain`: the user-managed `capture/` in-basket
/// plus the four PARA buckets. The declaration order is the display order in
/// the picker (`Ord` derives lexicographic enum order).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Bucket {
    Capture,
    Projects,
    Areas,
    Resources,
    Archive,
}

impl Bucket {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Capture => "Capture",
            Self::Projects => "Projects",
            Self::Areas => "Areas",
            Self::Resources => "Resources",
            Self::Archive => "Archive",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// Absolute path on disk. Passed to `open` when the user selects.
    pub path: PathBuf,
    /// `~/brain/...` form for display + fuzzy matching.
    pub display: String,
    /// Which PARA bucket this entry belongs to.
    pub bucket: Bucket,
    /// Whether this entry is a directory. Recorded during the walk, where
    /// `walkdir` already knows, so consumers (the tree, the picker's palette
    /// context) never need a syscall to ask.
    pub is_dir: bool,
}

/// Collect pickable entries under each root.
///
/// `brain` is the absolute path to `~/brain`, used to rewrite paths into
/// `~/brain/...` form. `roots` pairs each root directory with the bucket
/// label to apply to entries found there. Missing roots are silently skipped.
pub fn collect(brain: &Path, roots: &[(Bucket, PathBuf)]) -> Result<Vec<Entry>> {
    let mut out: Vec<Entry> = Vec::new();
    for (bucket, root) in roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root)
            .into_iter()
            .filter_entry(|e| !is_hidden(e))
        {
            let entry = entry.with_context(|| format!("walking {}", root.display()))?;
            // Skip the root itself; it's not pickable.
            if entry.depth() == 0 {
                continue;
            }
            let display = display_path(brain, entry.path());
            let is_dir = entry.file_type().is_dir();
            out.push(Entry {
                path: entry.into_path(),
                display,
                bucket: *bucket,
                is_dir,
            });
        }
    }
    Ok(out)
}

fn is_hidden(e: &DirEntry) -> bool {
    e.depth() > 0 && e.file_name().to_str().is_some_and(|n| n.starts_with('.'))
}

fn display_path(brain: &Path, path: &Path) -> String {
    // Rewrite `$HOME/brain/...` → `~/brain/...`.
    if let Some(home) = brain.parent()
        && let Ok(rel) = path.strip_prefix(home)
    {
        return format!("~/{}", rel.display());
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_rewrites_home_prefix_to_tilde() {
        let brain = Path::new("/Users/x/brain");
        let path = Path::new("/Users/x/brain/projects/foo/note.md");
        assert_eq!(display_path(brain, path), "~/brain/projects/foo/note.md");
    }

    #[test]
    fn display_falls_back_to_absolute_outside_home() {
        // A path that doesn't sit under brain's parent is shown verbatim.
        let brain = Path::new("/Users/x/brain");
        let path = Path::new("/etc/hosts");
        assert_eq!(display_path(brain, path), "/etc/hosts");
    }

    #[test]
    fn bucket_labels_are_stable() {
        assert_eq!(Bucket::Capture.label(), "Capture");
        assert_eq!(Bucket::Projects.label(), "Projects");
        assert_eq!(Bucket::Areas.label(), "Areas");
        assert_eq!(Bucket::Resources.label(), "Resources");
        assert_eq!(Bucket::Archive.label(), "Archive");
    }

    #[test]
    fn bucket_display_order_is_capture_projects_areas_resources_archive() {
        // The picker relies on this ordering (derived `Ord`) to group
        // sections Capture → P → A → R → Archive. Capture leads because it is
        // the unprocessed in-basket the user just dumped into; Archive trails
        // because it is retired material.
        let mut order = [
            Bucket::Archive,
            Bucket::Resources,
            Bucket::Projects,
            Bucket::Areas,
            Bucket::Capture,
        ];
        order.sort_unstable();
        assert_eq!(
            order,
            [
                Bucket::Capture,
                Bucket::Projects,
                Bucket::Areas,
                Bucket::Resources,
                Bucket::Archive
            ]
        );
    }

    #[test]
    fn collect_records_whether_each_entry_is_a_directory() {
        let temp = tempfile::tempdir().expect("temp dir");
        let brain = temp.path();
        let projects = brain.join("projects");
        std::fs::create_dir_all(projects.join("atlas")).expect("create nested dir");
        std::fs::write(projects.join("atlas/plan.md"), "# plan").expect("write file");

        let entries = collect(brain, &[(Bucket::Projects, projects)]).expect("collect");

        let dir = entries
            .iter()
            .find(|e| e.path.ends_with("atlas"))
            .expect("the atlas directory is collected");
        let file = entries
            .iter()
            .find(|e| e.path.ends_with("plan.md"))
            .expect("the plan file is collected");

        assert!(dir.is_dir, "a directory entry must be marked as one");
        assert!(!file.is_dir, "a file entry must not be marked as a directory");
    }
}
