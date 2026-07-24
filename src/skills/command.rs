//! `brain skills sync [--root <dir>]` — render + install the bundled skills.
//!
//! With `--root`, everything installs under a sandbox dir (dev/tests, no touch to
//! the live registry). Without it, the real per-user layout is used.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use super::install;
use super::layout::Layout;

/// Run `brain skills sync`. `root` (from `--root`) selects a sandbox layout.
pub fn run_sync(root: Option<&Path>) -> Result<()> {
    let layout = if let Some(r) = root {
        Layout::under_root(r)
    } else {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("$HOME is not set"))?;
        Layout::real(&home)
    };
    let report = install::sync(&layout)?;
    println!(
        "synced {} skill(s): {}",
        report.installed.len(),
        report.installed.join(", ")
    );
    Ok(())
}
