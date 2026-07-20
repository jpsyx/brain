//! Brain-root resolution: where `~/brain` actually lives.
//!
//! Resolution order: first the `root` field of `config.json` next to the
//! project root (tilde-expanded), otherwise `$HOME/brain`. The resolved
//! directory must exist or `brain_root` errors.
//!
//! The IO-free pieces (`parse_config_root`, `expand_tilde_with_home`) are
//! split out from the env/filesystem-touching wrappers so they can be unit
//! tested without a real `$HOME`, a real `config.json`, or a real exe path.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};

/// Resolve the absolute `~/brain` directory, erroring if it does not exist.
pub fn brain_root() -> Result<PathBuf> {
    let configured = config_root()?;
    let brain = match configured {
        Some(raw) => expand_tilde(&raw)?,
        None => default_brain_root()?,
    };
    if !brain.is_dir() {
        bail!("{} does not exist", brain.display());
    }
    Ok(brain)
}

fn default_brain_root() -> Result<PathBuf> {
    let home = home_dir()?;
    Ok(home.join("brain"))
}

/// Read `config.json` sitting next to the project root (three levels up
/// from `target/release/brain`). Returns the raw `root` string verbatim
/// — tilde expansion happens in the caller. A missing file is not an error.
fn config_root() -> Result<Option<String>> {
    let Some(path) = config_path() else {
        return Ok(None);
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    parse_config_root(&text).map_err(|e| anyhow!("failed to parse {}: {e}", path.display()))
}

/// Parse the `root` field out of a `config.json` body. Pure: no IO. An
/// empty string is treated as "unset" (`None`) so a blank config falls back
/// to the default root.
pub fn parse_config_root(text: &str) -> Result<Option<String>> {
    #[derive(serde::Deserialize)]
    struct Config {
        root: Option<String>,
    }
    let cfg: Config = serde_json::from_str(text)?;
    Ok(cfg.root.filter(|s| !s.is_empty()))
}

fn config_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // `<root>/target/release/brain` → `<root>/config.json`
    let project_root = exe.parent()?.parent()?.parent()?;
    Some(project_root.join("config.json"))
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
    fn parse_reads_root_field() {
        let got = parse_config_root(r#"{"root": "~/brain"}"#).unwrap();
        assert_eq!(got, Some("~/brain".to_owned()));
    }

    #[test]
    fn empty_root_is_treated_as_unset() {
        assert_eq!(parse_config_root(r#"{"root": ""}"#).unwrap(), None);
    }

    #[test]
    fn missing_root_field_is_none() {
        assert_eq!(parse_config_root("{}").unwrap(), None);
    }

    #[test]
    fn null_root_is_none() {
        assert_eq!(parse_config_root(r#"{"root": null}"#).unwrap(), None);
    }

    #[test]
    fn invalid_json_errors() {
        assert!(parse_config_root("not json").is_err());
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
}
