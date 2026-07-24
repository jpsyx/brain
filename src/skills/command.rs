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
    let report = install::sync(&layout, &sources)?;
    println!(
        "synced {} skill(s): {}",
        report.installed.len(),
        report.installed.join(", ")
    );
    Ok(())
}
