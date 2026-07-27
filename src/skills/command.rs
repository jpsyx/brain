//! `brain skills sync [--root <dir>]` — render + install the bundled skills
//! (plus the user's extensions/plugins).
//!
//! With `--root`, everything installs under a sandbox dir and reads
//! extensions/plugins from `<root>/{extensions,plugins}` (dev/tests, no touch to
//! the live registry). Without it, the real per-user layout + brain-root sources
//! are used.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use super::install::{self, Sources};
use super::layout::Layout;

/// Run `brain skills sync`. `root` (from `--root`) selects a sandbox.
pub fn run_sync(root: Option<&Path>) -> Result<()> {
    let (layout, sources) = if let Some(r) = root {
        (
            Layout::under_root(r),
            Sources {
                extensions_dir: Some(r.join("extensions")),
                plugins_dir: Some(r.join("plugins")),
            },
        )
    } else {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("$HOME is not set"))?;
        (Layout::real(&home), super::real_sources())
    };
    let theme = crate::theme::Theme::active();
    eprintln!("{}", format_sync_plan(&layout, &sources, theme));
    crate::logging::log(format!(
        "skills sync built={} registry={} frontends={}",
        layout.built_dir.display(),
        layout.agents_dir.display(),
        layout.frontends.len()
    ));
    let report = install::sync(&layout, &sources)?;
    crate::logging::log(format!("skills sync installed={}", report.installed.len()));
    println!(
        "{} {}",
        theme.success(&format!("synced {} skill(s):", report.installed.len())),
        theme.muted(&report.installed.join(", "))
    );
    Ok(())
}

#[must_use]
pub fn format_sync_plan(layout: &Layout, sources: &Sources, theme: crate::theme::Theme) -> String {
    let extensions = sources
        .extensions_dir
        .as_ref()
        .map_or_else(|| "none".to_owned(), |p| p.display().to_string());
    let plugins = sources
        .plugins_dir
        .as_ref()
        .map_or_else(|| "none".to_owned(), |p| p.display().to_string());
    format!(
        "{}\n  {} {}\n  {} {}\n  {} {}\n  {} {}\n  {} {}",
        theme.heading("Rendering and installing brain skills"),
        theme.muted("built:"),
        theme.value(&layout.built_dir.display().to_string()),
        theme.muted("registry:"),
        theme.value(&layout.agents_dir.display().to_string()),
        theme.muted("frontends:"),
        theme.value(&layout.frontends.len().to_string()),
        theme.muted("extensions:"),
        theme.value(&extensions),
        theme.muted("plugins:"),
        theme.value(&plugins),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_plan_names_skill_install_destinations() {
        let layout = Layout::under_root(Path::new("/tmp/brain-skills"));
        let sources = Sources {
            extensions_dir: Some(PathBuf::from("/tmp/brain-skills/extensions")),
            plugins_dir: Some(PathBuf::from("/tmp/brain-skills/plugins")),
        };

        let plan = format_sync_plan(&layout, &sources, crate::theme::Theme::dark(false));

        assert!(
            plan.contains("Rendering and installing brain skills"),
            "{plan}"
        );
        assert!(plan.contains("built: /tmp/brain-skills/built"), "{plan}");
        assert!(
            plan.contains("registry: /tmp/brain-skills/agents/skills"),
            "{plan}"
        );
        assert!(plan.contains("frontends: 4"), "{plan}");
        assert!(
            plan.contains("extensions: /tmp/brain-skills/extensions"),
            "{plan}"
        );
        assert!(
            plan.contains("plugins: /tmp/brain-skills/plugins"),
            "{plan}"
        );
    }
}
