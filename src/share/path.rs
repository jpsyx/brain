use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// Resolve relative inputs against the caller's cwd, within the selected workspace.
pub fn resolve_path(input: &Path, cwd: &Path, root: &Path) -> Result<PathBuf> {
    if input.as_os_str().is_empty() {
        bail!("That is not a valid file or directory path");
    }
    let candidate = if input.is_absolute() {
        input.to_path_buf()
    } else {
        cwd.join(input)
    };
    let path = candidate
        .canonicalize()
        .with_context(|| format!("{} is not a valid file or directory path", input.display()))?;
    if !path.is_file() && !path.is_dir() {
        bail!("{} is not a valid file or directory path", input.display());
    }
    let root = root
        .canonicalize()
        .context("Resolve selected workspace root")?;
    if !path.starts_with(&root) {
        bail!(
            "Choose a file or directory inside the selected workspace: {}",
            root.display()
        );
    }
    Ok(path)
}
