//! Skill install destinations and the link targets between them.
//!
//! Mirrors the jpsyx link structure so brain-owned skills coexist with it: a
//! built canonical dir, the shared `~/.agents/skills` registry linking to it,
//! and each frontend's skills dir linking to the registry.
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
    /// Canonical dir the rendered skills are written to (link targets resolve here).
    pub built_dir: PathBuf,
    /// The shared agent registry (`~/.agents/skills`).
    pub agents_dir: PathBuf,
    /// Each installed frontend's skills dir (Claude, Codex, OpenCode, Cursor).
    pub frontends: Vec<PathBuf>,
}

impl Layout {
    /// A self-contained sandbox layout under `root` (for dev/tests): built,
    /// registry, and a fixed set of frontend dirs all mirrored beneath it. Never
    /// touches the real user dirs.
    #[must_use]
    pub fn under_root(root: &Path) -> Self {
        Self {
            built_dir: root.join("built"),
            agents_dir: root.join("agents").join("skills"),
            frontends: ["claude", "codex", "opencode", "cursor"]
                .iter()
                .map(|f| root.join(f).join("skills"))
                .collect(),
        }
    }

    /// The real per-user layout, including only frontends whose base dir exists
    /// (so we don't create skill dirs for uninstalled frontends). Codex is a
    /// required target alongside Claude.
    #[must_use]
    pub fn real(home: &Path) -> Self {
        let data = std::env::var_os("XDG_DATA_HOME")
            .filter(|s| !s.is_empty())
            .map_or_else(|| home.join(".local").join("share"), PathBuf::from);
        let candidates = [
            home.join(".claude").join("skills"),
            home.join(".codex").join("skills"),
            home.join(".config").join("opencode").join("skills"),
            home.join(".cursor").join("skills-cursor"),
        ];
        let frontends = candidates
            .into_iter()
            .filter(|d| d.parent().is_some_and(Path::exists))
            .collect();
        Self {
            built_dir: data.join("brain").join("skills"),
            agents_dir: home.join(".agents").join("skills"),
            frontends,
        }
    }
}

/// The links needed to install skill `name`: the registry entry pointing at the
/// built dir, then each frontend pointing at the registry. Pure.
#[must_use]
pub fn link_ops(name: &str, layout: &Layout) -> Vec<Link> {
    let registry = layout.agents_dir.join(name);
    let mut ops = vec![Link {
        link_path: registry.clone(),
        target: layout.built_dir.join(name),
    }];
    for f in &layout.frontends {
        ops.push(Link {
            link_path: f.join(name),
            target: registry.clone(),
        });
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_ops_registry_points_at_built_then_frontends_point_at_registry() {
        let layout = Layout::under_root(Path::new("/tmp/sbx"));
        let ops = link_ops("article-summarizer", &layout);
        // First op: registry entry → built dir.
        assert_eq!(
            ops[0],
            Link {
                link_path: PathBuf::from("/tmp/sbx/agents/skills/article-summarizer"),
                target: PathBuf::from("/tmp/sbx/built/article-summarizer"),
            }
        );
        // The rest: each frontend → the registry entry (never the built dir).
        let registry = PathBuf::from("/tmp/sbx/agents/skills/article-summarizer");
        for op in &ops[1..] {
            assert_eq!(op.target, registry);
            assert!(op.link_path.ends_with("skills/article-summarizer"));
        }
        // Claude + Codex + OpenCode + Cursor.
        assert_eq!(ops.len(), 1 + 4);
    }

    #[test]
    fn link_ops_covers_codex() {
        let layout = Layout::under_root(Path::new("/tmp/sbx"));
        let ops = link_ops("x", &layout);
        assert!(
            ops.iter()
                .any(|o| o.link_path.as_path() == Path::new("/tmp/sbx/codex/skills/x")),
            "Codex must be a fan-out target"
        );
    }
}
