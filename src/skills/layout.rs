//! Skill install destinations and the link targets between them.
//!
//! Rendered skills live in the selected brain workspace and are exposed through
//! project-local frontend skill directories.
//!
//! `link_ops` (the target computation) is pure and unit-tested; `real` /
//! `under_root` are the IO-flavored constructors.

use std::path::{Path, PathBuf};

/// One symlink to create: `link_path` → `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub link_path: PathBuf,
    pub target: PathBuf,
}

/// The install destinations for a `brain skills sync`.
#[derive(Debug, Clone)]
pub struct Layout {
    /// Canonical dir the rendered skills are written to.
    pub built_dir: PathBuf,
    /// The workspace-local `.agents/skills` directory.
    pub agents_dir: PathBuf,
    /// Each project-local frontend's skills dir (Claude, Codex, OpenCode).
    pub frontends: Vec<PathBuf>,
}

/// Cache-local actor capability render with no registry or frontend links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCapabilityLayout {
    pub built_dir: PathBuf,
}

impl Layout {
    /// A workspace-local layout under `root` (for dev/tests).
    #[must_use]
    pub fn under_root(root: &Path) -> Self {
        Self {
            built_dir: root.join(".agents").join("skills"),
            agents_dir: root.join(".agents").join("skills"),
            frontends: [".claude", ".codex", ".opencode"]
                .iter()
                .map(|f| root.join(f).join("skills"))
                .collect(),
        }
    }

    /// The real layout for a selected brain workspace. All supported project
    /// frontends are targets, even when their directories do not yet exist.
    #[must_use]
    pub fn real(root: &Path) -> Self {
        Self::under_root(root)
    }

    /// Selected workspace/actor render destination under the UUID cache.
    #[must_use]
    pub fn workspace_capabilities(
        workspace: &crate::workspace::WorkspaceContext,
        actor: &crate::actor::ActorContext,
    ) -> WorkspaceCapabilityLayout {
        WorkspaceCapabilityLayout {
            built_dir: workspace.paths().capability_skills_dir(actor.user_id()),
        }
    }
}

/// The links needed to expose skill `name` from `.agents/skills` to each
/// project-local frontend. Pure.
#[must_use]
pub fn link_ops(name: &str, layout: &Layout) -> Vec<Link> {
    layout
        .frontends
        .iter()
        .map(|frontend| Link {
            link_path: frontend.join(name),
            target: layout.agents_dir.join(name),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_ops_frontends_point_at_workspace_agents_skill() {
        let layout = Layout::under_root(Path::new("/tmp/sbx"));
        let ops = link_ops("article-summarizer", &layout);
        let skill = PathBuf::from("/tmp/sbx/.agents/skills/article-summarizer");
        for op in &ops {
            assert_eq!(op.target, skill);
            assert!(op.link_path.ends_with("skills/article-summarizer"));
        }
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn link_ops_covers_codex() {
        let layout = Layout::under_root(Path::new("/tmp/sbx"));
        let ops = link_ops("x", &layout);
        assert!(
            ops.iter()
                .any(|o| o.link_path.as_path() == Path::new("/tmp/sbx/.codex/skills/x")),
            "Codex must be a fan-out target"
        );
    }

    #[test]
    fn workspace_layout_targets_only_project_frontends() {
        let layout = Layout::real(Path::new("/tmp/brain-root"));

        assert_eq!(
            layout.agents_dir,
            Path::new("/tmp/brain-root/.agents/skills")
        );
        assert_eq!(
            layout.frontends,
            vec![
                PathBuf::from("/tmp/brain-root/.claude/skills"),
                PathBuf::from("/tmp/brain-root/.codex/skills"),
                PathBuf::from("/tmp/brain-root/.opencode/skills"),
            ]
        );
    }
}
