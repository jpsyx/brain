//! Removing what a sync no longer produces.
//!
//! Every skill directory brain renders carries a marker file, so a later sync
//! can tell its own output apart from a skill the user wrote by hand in the
//! workspace skills directory. A rendered skill the current sync no longer
//! produces (a deleted or renamed plugin, a bundled skill dropped from the
//! binary) is removed along with its frontend links; an unmarked directory is
//! the user's and is never touched.
//!
//! The two decisions are pure and unit-tested; `run` is the thin FS shell.

use std::fs;
use std::path::Path;

use anyhow::Result;

use super::layout::{Layout, link_ops};

/// Written into every skill directory brain renders. Its presence is what
/// makes a directory brain's to delete.
pub const RENDERED_MARKER: &str = ".brain-rendered";

/// Previously rendered skills the current sync did not produce. Pure.
#[must_use]
pub fn stale_rendered(rendered: &[String], installed: &[String]) -> Vec<String> {
    rendered
        .iter()
        .filter(|name| !installed.iter().any(|kept| kept == *name))
        .cloned()
        .collect()
}

/// Whether a frontend entry is a brain-owned link to a skill that is gone. Pure.
#[must_use]
pub fn is_orphan_frontend_link(target: &Path, agents_dir: &Path, target_exists: bool) -> bool {
    !target_exists && target.starts_with(agents_dir)
}

/// Remove every rendered skill the current sync did not produce, plus any
/// frontend link left dangling by a skill that no longer exists. Returns the
/// pruned skill names.
pub(super) fn run(layout: &Layout, installed: &[String]) -> Result<Vec<String>> {
    let stale = stale_rendered(&rendered_names(&layout.built_dir), installed);
    for name in &stale {
        crate::logging::log(format!("skills prune {name}"));
        remove_existing(&layout.built_dir.join(name))?;
        for link in link_ops(name, layout) {
            remove_existing(&link.link_path)?;
        }
    }
    for frontend in &layout.frontends {
        sweep_orphan_links(frontend, &layout.agents_dir)?;
    }
    Ok(stale)
}

/// The skill directories under `built_dir` that brain rendered.
fn rendered_names(built_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(built_dir) else {
        return Vec::new();
    };
    let mut names = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return None;
            }
            if path.join(RENDERED_MARKER).is_file() {
                entry.file_name().into_string().ok()
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn sweep_orphan_links(frontend: &Path, agents_dir: &Path) -> Result<()> {
    let Ok(entries) = fs::read_dir(frontend) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_symlink() {
            continue;
        }
        let Ok(target) = fs::read_link(&path) else {
            continue;
        };
        if is_orphan_frontend_link(&target, agents_dir, target.exists()) {
            crate::logging::log(format!("skills prune link {}", path.display()));
            remove_existing(&path)?;
        }
    }
    Ok(())
}

/// Remove whatever sits at `path` (symlink, file, or dir). `symlink_metadata`
/// does not follow the link, so a dangling symlink is handled too.
pub(super) fn remove_existing(path: &Path) -> Result<()> {
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
    use std::path::PathBuf;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn stale_rendered_is_what_the_previous_render_left_behind() {
        let rendered = names(&["kept", "renamed-away"]);
        let installed = names(&["kept", "brand-new"]);

        assert_eq!(
            stale_rendered(&rendered, &installed),
            names(&["renamed-away"])
        );
    }

    #[test]
    fn nothing_is_stale_when_every_rendered_skill_is_still_produced() {
        let rendered = names(&["a", "b"]);

        assert!(stale_rendered(&rendered, &names(&["b", "a", "c"])).is_empty());
    }

    #[test]
    fn a_user_authored_skill_is_never_stale_because_it_was_never_rendered() {
        // `rendered` only ever holds marked directories, so a hand-written
        // skill cannot appear in it — the empty input is the whole guarantee.
        assert!(stale_rendered(&[], &names(&["bundled"])).is_empty());
    }

    #[test]
    fn a_frontend_link_into_the_workspace_is_orphaned_once_its_skill_is_gone() {
        let agents = Path::new("/root/.agents/skills");

        assert!(is_orphan_frontend_link(
            &PathBuf::from("/root/.agents/skills/gone"),
            agents,
            false
        ));
    }

    #[test]
    fn a_live_link_and_a_foreign_link_are_both_left_alone() {
        let agents = Path::new("/root/.agents/skills");

        assert!(!is_orphan_frontend_link(
            &PathBuf::from("/root/.agents/skills/live"),
            agents,
            true
        ));
        assert!(!is_orphan_frontend_link(
            &PathBuf::from("/elsewhere/skills/theirs"),
            agents,
            false
        ));
    }
}
