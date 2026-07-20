//! Integration coverage for `entry::collect` against a real directory tree.
//!
//! `collect` is the one place `brain` touches the filesystem in bulk, so we
//! exercise it through real temp dirs rather than mocking walkdir: hidden
//! filtering, root-skipping, `~/brain/...` display rewriting, bucket
//! tagging, and tolerance of a missing bucket.

use std::fs;
use std::path::{Path, PathBuf};

use brain::entry::{self, Bucket};

/// Build a fake `$HOME/brain` under `tmp` and return (home, brain).
fn make_brain(tmp: &Path) -> (PathBuf, PathBuf) {
    let home = tmp.join("home");
    let brain = home.join("brain");
    for bucket in ["projects", "areas", "resources"] {
        fs::create_dir_all(brain.join(bucket)).unwrap();
    }
    (home, brain)
}

fn touch(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, b"x").unwrap();
}

fn displays(entries: &[entry::Entry]) -> Vec<String> {
    entries.iter().map(|e| e.display.clone()).collect()
}

#[test]
fn collects_files_and_dirs_tagged_by_bucket() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (_home, brain) = make_brain(tmp.path());
    touch(&brain.join("projects/alpha/plan.md"));
    touch(&brain.join("areas/health/log.md"));

    let roots = vec![
        (Bucket::Projects, brain.join("projects")),
        (Bucket::Areas, brain.join("areas")),
        (Bucket::Resources, brain.join("resources")),
    ];
    let entries = entry::collect(&brain, &roots).unwrap();

    // Every project-rooted entry is tagged Projects, etc.
    for e in &entries {
        let want = match e.bucket {
            Bucket::Projects => "/projects/",
            Bucket::Areas => "/areas/",
            Bucket::Resources => "/resources/",
            Bucket::Archive => "/archive/",
        };
        assert!(
            e.display.contains(want),
            "{} tagged {:?}",
            e.display,
            e.bucket
        );
    }

    let ds = displays(&entries);
    assert!(ds.iter().any(|d| d.ends_with("projects/alpha/plan.md")));
    assert!(ds.iter().any(|d| d.ends_with("projects/alpha"))); // the dir itself
    assert!(ds.iter().any(|d| d.ends_with("areas/health/log.md")));
}

#[test]
fn display_paths_use_tilde_brain_form() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (_home, brain) = make_brain(tmp.path());
    touch(&brain.join("projects/alpha/plan.md"));

    let roots = vec![(Bucket::Projects, brain.join("projects"))];
    let entries = entry::collect(&brain, &roots).unwrap();

    // brain.parent() stands in for $HOME, so the rewrite is `~/brain/...`.
    assert!(
        entries.iter().all(|e| e.display.starts_with("~/brain/")),
        "got: {:?}",
        displays(&entries)
    );
}

#[test]
fn hidden_files_and_dirs_are_skipped() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (_home, brain) = make_brain(tmp.path());
    touch(&brain.join("projects/visible.md"));
    touch(&brain.join("projects/.hidden.md"));
    touch(&brain.join("projects/.git/config"));

    let roots = vec![(Bucket::Projects, brain.join("projects"))];
    let ds = displays(&entry::collect(&brain, &roots).unwrap());

    assert!(ds.iter().any(|d| d.ends_with("projects/visible.md")));
    assert!(!ds.iter().any(|d| d.contains(".hidden")));
    assert!(!ds.iter().any(|d| d.contains(".git")));
}

#[test]
fn the_root_directory_itself_is_not_pickable() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (_home, brain) = make_brain(tmp.path());
    touch(&brain.join("projects/note.md"));

    let projects = brain.join("projects");
    let roots = vec![(Bucket::Projects, projects.clone())];
    let entries = entry::collect(&brain, &roots).unwrap();

    assert!(
        entries.iter().all(|e| e.path != projects),
        "the bucket root should be filtered out"
    );
}

#[test]
fn a_missing_bucket_is_silently_skipped() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (_home, brain) = make_brain(tmp.path());
    touch(&brain.join("projects/note.md"));

    let roots = vec![
        (Bucket::Projects, brain.join("projects")),
        (Bucket::Areas, brain.join("does-not-exist")),
    ];
    // No error despite the missing root; we just get the projects entries.
    let entries = entry::collect(&brain, &roots).unwrap();
    assert!(entries.iter().all(|e| e.bucket == Bucket::Projects));
    assert!(!entries.is_empty());
}
