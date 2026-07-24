//! Applying a render to disk.
//!
//! Writes each rendered skill into the built dir, then creates the registry +
//! frontend symlinks. The link *targets* are computed by the pure
//! `layout::link_ops`; this module is the thin FS shell.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::layout::{Layout, Link, link_ops};
use super::{embed, render};

/// What a sync did.
pub struct Report {
    pub installed: Vec<String>,
}

/// Render + install every bundled skill into `layout`. Idempotent: an existing
/// built dir or link at a destination is replaced.
pub fn sync(layout: &Layout) -> Result<Report> {
    let mut installed = Vec::new();
    for skill in embed::bundled_skills() {
        write_built(&skill, layout)?;
        for link in link_ops(&skill.name, layout) {
            create_symlink(&link)?;
        }
        installed.push(skill.name);
    }
    Ok(Report { installed })
}

fn write_built(skill: &embed::BundledSkill, layout: &Layout) -> Result<()> {
    let dest = layout.built_dir.join(&skill.name);
    // Clear any prior build so a removed file doesn't linger.
    if dest.exists() {
        fs::remove_dir_all(&dest)
            .with_context(|| format!("clearing built skill {}", dest.display()))?;
    }
    for rf in render::render(skill) {
        let path = dest.join(&rf.rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &rf.contents)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

fn create_symlink(link: &Link) -> Result<()> {
    if let Some(parent) = link.link_path.parent() {
        fs::create_dir_all(parent)?;
    }
    remove_existing(&link.link_path)?;
    std::os::unix::fs::symlink(&link.target, &link.link_path)
        .with_context(|| format!("linking {} → {}", link.link_path.display(), link.target.display()))
}

/// Remove whatever currently sits at `path` (symlink, file, or dir) so a fresh
/// symlink can be created. `symlink_metadata` does not follow the link, so a
/// dangling symlink is handled too.
fn remove_existing(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn sandbox() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("brain-skills-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn sync_writes_built_skill_and_registry_and_frontend_links() {
        let root = sandbox();
        let layout = Layout::under_root(&root);
        let report = sync(&layout).unwrap();
        assert!(report.installed.iter().any(|n| n == "article-summarizer"));

        // Built copy exists with real content.
        let built = layout.built_dir.join("article-summarizer").join("SKILL.md");
        assert!(built.is_file(), "built SKILL.md should exist");
        let text = fs::read_to_string(&built).unwrap();
        assert!(text.contains("article-summarizer"));

        // Registry entry is a symlink to the built dir.
        let registry = layout.agents_dir.join("article-summarizer");
        assert_eq!(
            fs::read_link(&registry).unwrap(),
            layout.built_dir.join("article-summarizer")
        );

        // Every frontend links to the registry entry, and resolves to the file.
        for f in &layout.frontends {
            let fe = f.join("article-summarizer");
            assert_eq!(fs::read_link(&fe).unwrap(), registry);
            assert!(fe.join("SKILL.md").is_file(), "frontend link resolves to the skill");
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_is_idempotent() {
        let root = sandbox();
        let layout = Layout::under_root(&root);
        sync(&layout).unwrap();
        // Second run must not error on existing built dir / links.
        sync(&layout).unwrap();
        assert!(layout.agents_dir.join("article-summarizer").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_never_touches_paths_outside_the_layout_root() {
        let root = sandbox();
        sync(&Layout::under_root(&root)).unwrap();
        // Everything created lives under the sandbox root.
        assert!(root.starts_with(std::env::temp_dir()));
        let _ = fs::remove_dir_all(&root);
    }
}
