//! Brain-root resolution: where the brain (PARA) directory lives.
//!
//! Resolution order: the path written in `~/.config/brain-root` (tilde-expanded)
//! if that file exists and is non-empty, otherwise the default `$HOME/brain`.
//!
//! `brain-root` is the **one** machine-local pointer brain needs, and the *only*
//! thing that can't live inside the brain root itself (you can't store the
//! brain's location inside the brain — that's circular). It is edited by hand
//! (or tracked externally, e.g. via jpsyx), never by a `brain` CLI command — it
//! is deliberately not a `brain config` variable. Everything else brain persists
//! lives *inside* the resolved root at `<root>/.config/` (config.json,
//! personalization.json, extensions/, plugins/), so it travels with the brain.
//!
//! The IO-free pieces (`parse_brain_root_file`, `expand_tilde_with_home`) are
//! split out from the env/filesystem-touching wrappers so they can be unit
//! tested without a real `$HOME` or a real `brain-root` file.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};

/// Resolve the absolute brain-root directory, erroring if it does not exist.
pub fn brain_root() -> Result<PathBuf> {
    let root = brain_root_path();
    if !root.is_dir() {
        bail!("{} does not exist", root.display());
    }
    Ok(root)
}

/// Resolve the brain-root path **without** requiring it to exist.
///
/// Used to derive the config dir (`<root>/.config`), where a missing dir must
/// read as empty rather than fail — config lookups must never block startup.
#[must_use]
pub fn brain_root_path() -> PathBuf {
    read_brain_root_file().map_or_else(default_brain_root_path, |raw| {
        expand_tilde(&raw).unwrap_or_else(|_| PathBuf::from(raw))
    })
}

/// `$HOME/brain` (best-effort: an unset `$HOME` yields a relative `brain`).
fn default_brain_root_path() -> PathBuf {
    home_dir().unwrap_or_default().join("brain")
}

/// The machine-local brain-root pointer file: `$XDG_CONFIG_HOME/brain-root` or
/// `~/.config/brain-root`. This is a plain `$HOME`-side path (NOT inside the
/// brain root), so reading it never depends on the root it resolves.
fn brain_root_file() -> PathBuf {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map_or_else(|| home_dir().unwrap_or_default().join(".config"), PathBuf::from);
    config_home.join("brain-root")
}

/// The machine-local brain-env directory.
///
/// `$XDG_CONFIG_HOME/brain` or `~/.config/brain`. It holds `env.json` (brain
/// env). Unlike the brain-internal config dir it lives at a fixed `$HOME`-side
/// path that does **not** depend on the brain root, so it can hold `root`
/// itself without circularity and never rides the brain-dir sync.
#[must_use]
pub fn machine_config_dir() -> PathBuf {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok().filter(|s| !s.is_empty());
    let home = home_dir().unwrap_or_default();
    machine_config_dir_from(xdg.as_deref(), &home)
}

/// Pure core of [`machine_config_dir`]: `<xdg>/brain`, else `<home>/.config/brain`.
#[must_use]
pub fn machine_config_dir_from(xdg_config_home: Option<&str>, home: &Path) -> PathBuf {
    let base = xdg_config_home
        .filter(|s| !s.is_empty())
        .map_or_else(|| home.join(".config"), PathBuf::from);
    base.join("brain")
}

/// Read + parse the brain-root pointer. `None` when the file is absent, empty,
/// or unreadable (so the caller falls back to the default root).
fn read_brain_root_file() -> Option<String> {
    std::fs::read_to_string(brain_root_file())
        .ok()
        .as_deref()
        .and_then(parse_brain_root_file)
}

/// Parse the contents of a `brain-root` file into a path string. Pure: no IO.
/// The file holds a single path; surrounding whitespace/newlines are trimmed and
/// an empty file is treated as "unset" (`None`).
#[must_use]
pub fn parse_brain_root_file(contents: &str) -> Option<String> {
    let trimmed = contents.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Expand a leading `~` / `~/` against `$HOME`. Non-tilde paths pass through.
pub fn expand_tilde(raw: &str) -> Result<PathBuf> {
    if raw == "~" || raw.starts_with("~/") {
        let home = home_dir()?;
        return Ok(expand_tilde_with_home(raw, &home));
    }
    Ok(PathBuf::from(raw))
}

/// Pure tilde expansion against an explicit home directory.
#[must_use]
pub fn expand_tilde_with_home(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(raw)
}

fn home_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("$HOME is not set"))?;
    Ok(PathBuf::from(home))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_the_path_line() {
        assert_eq!(parse_brain_root_file("~/brain"), Some("~/brain".to_owned()));
        assert_eq!(parse_brain_root_file("/srv/brain"), Some("/srv/brain".to_owned()));
    }

    #[test]
    fn parse_trims_surrounding_whitespace_and_newlines() {
        assert_eq!(parse_brain_root_file("  ~/brain \n"), Some("~/brain".to_owned()));
    }

    #[test]
    fn empty_or_blank_file_is_unset() {
        assert_eq!(parse_brain_root_file(""), None);
        assert_eq!(parse_brain_root_file("   \n\t"), None);
    }

    #[test]
    fn tilde_slash_expands_against_home() {
        let home = Path::new("/Users/x");
        assert_eq!(
            expand_tilde_with_home("~/brain/projects", home),
            PathBuf::from("/Users/x/brain/projects")
        );
    }

    #[test]
    fn bare_tilde_is_home() {
        assert_eq!(
            expand_tilde_with_home("~", Path::new("/Users/x")),
            PathBuf::from("/Users/x")
        );
    }

    #[test]
    fn absolute_path_passes_through_untouched() {
        assert_eq!(
            expand_tilde_with_home("/etc/brain", Path::new("/Users/x")),
            PathBuf::from("/etc/brain")
        );
    }

    #[test]
    fn mid_string_tilde_is_not_expanded() {
        // Only a *leading* ~ is a home reference.
        assert_eq!(
            expand_tilde_with_home("/a/~/b", Path::new("/Users/x")),
            PathBuf::from("/a/~/b")
        );
    }

    #[test]
    fn machine_config_dir_prefers_xdg_config_home() {
        assert_eq!(
            machine_config_dir_from(Some("/xdg"), Path::new("/Users/x")),
            PathBuf::from("/xdg/brain")
        );
    }

    #[test]
    fn machine_config_dir_falls_back_to_home_dotconfig() {
        assert_eq!(
            machine_config_dir_from(None, Path::new("/Users/x")),
            PathBuf::from("/Users/x/.config/brain")
        );
    }
}
