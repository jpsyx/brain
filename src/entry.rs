//! Walk one or more root directories and collect entries (files + dirs).
//!
//! Hidden files (`.git`, `.DS_Store`, anything starting with `.`) are skipped,
//! matching the `fd .` default that the previous zsh helper relied on. Each
//! entry is tagged with its `Bucket` so the picker can group results into
//! Projects / Areas / Resources sections.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::{DirEntry, WalkDir};

/// PARA-style top-level bucket inside `~/brain`. The declaration order is the
/// display order in the picker (`Ord` derives lexicographic enum order).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Bucket {
    Projects,
    Areas,
    Resources,
    Archive,
}

impl Bucket {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
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
            out.push(Entry {
                path: entry.into_path(),
                display,
                bucket: *bucket,
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
        assert_eq!(Bucket::Projects.label(), "Projects");
        assert_eq!(Bucket::Areas.label(), "Areas");
        assert_eq!(Bucket::Resources.label(), "Resources");
        assert_eq!(Bucket::Archive.label(), "Archive");
    }

    #[test]
    fn bucket_display_order_is_projects_areas_resources_archive() {
        // The picker relies on this ordering (derived `Ord`) to group
        // sections P → A → R → Archive, with Archive last as retired material.
        let mut order = [
            Bucket::Archive,
            Bucket::Resources,
            Bucket::Projects,
            Bucket::Areas,
        ];
        order.sort_unstable();
        assert_eq!(
            order,
            [
                Bucket::Projects,
                Bucket::Areas,
                Bucket::Resources,
                Bucket::Archive
            ]
        );
    }
}
